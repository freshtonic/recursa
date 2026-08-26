//! M4 source-aware generation for authored Node shapes.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use quote::{ToTokens as _, format_ident, quote};
use syn::ext::IdentExt as _;
use syn::spanned::Spanned as _;
use syn::{GenericArgument, GenericParam, Generics, Ident, PathArguments, Type, Visibility};

use crate::PrattPosition;
use crate::TypeShape;
use crate::build_diagnostic::{BuildReport, DiagnosticStage, GenerateError};
use crate::decision_plan::{
    ContainerOperation, DispatchKind, ExpectedRecipe, FailureEdgeKind, FollowRecipe,
};
use crate::lexical::{KeywordRecognition, LexicalMatcher, LexicalPriority, TokenKindIdentity};
use crate::lowering::ModuleId;
use crate::m3::{CargoOutput, M3Input, analyze_source_tree, internal_generate_error};
use crate::materialization::{
    ClosedType, MaterializedElement, MaterializedFixedAttachment, MaterializedNodeApplication,
    MaterializedNodeBody, MaterializedPrattLed, MaterializedPrattLedPosition, MaterializedPrattNud,
    MaterializedPrattNudPosition, MaterializedPrattVariant, MaterializedVariant,
};
use crate::resolution::{
    ResolvedFieldId, ResolvedFieldStyle, ResolvedGenericArgument, ResolvedNode, ResolvedNodeBody,
    ResolvedType, ResolvedTypeKind, SemanticTarget,
};
use crate::source_model::{ConfiguredTraitRecipe, PrettySpacing, TokenDefinitionKind};
use crate::source_syntax::{CaptureMode, CargoCfgSnapshot, span_range};
use crate::{AtomExpression, FirstPosition, KindSet, SiteId, TextEquivalence, TextPredicate};

const PRIVATE_PROTOCOL: u32 = 1;
const EMITTER_IDENTITY: &str = "recursa-m4-compound-v3";
const SELECTED_GENERATION: &str = "recursa-selected.rs";

/// One complete published logical output.
pub struct Generation {
    report: BuildReport,
}

impl Generation {
    /// Warnings retained on this successful Generation.
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }
}

struct StdoutCargo;

impl CargoOutput for StdoutCargo {
    fn emit(&mut self, instruction: String) {
        println!("{instruction}");
    }
}

/// Analyzes, renders, stages, and atomically selects one complete Generation.
pub fn generate(root: impl AsRef<Path>) -> Result<Generation, GenerateError> {
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "OUT_DIR is unavailable"))?;
    invalidate_selection(&out_dir).map_err(|detail| failure(root.as_ref(), &detail))?;
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "CARGO_MANIFEST_DIR is unavailable"))?;
    let cfg = CargoCfgSnapshot::from_cargo_environment(env::vars_os());
    let mut cargo = StdoutCargo;
    let success = analyze_source_tree(
        M3Input::new(
            &manifest_dir,
            root.as_ref(),
            crate::lowering::RustEdition::Rust2024,
            cfg.clone(),
        ),
        &mut cargo,
    )?;
    if success
        .analyzed
        .lowered
        .source_modeled
        .resolved()
        .build_inputs
        .diagrams
        && !cfg!(feature = "diagrams")
    {
        return Err(failure(
            root.as_ref(),
            "diagram generation was requested but recursa-codegen was built without its `diagrams` capability",
        ));
    }
    let plan = EmissionPlan::join(&success.analyzed, &cfg)
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    let rendered = render(&plan).map_err(|detail| failure(root.as_ref(), &detail))?;
    publish(&out_dir, &plan.identity, &rendered)
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    Ok(Generation {
        report: success.build_report,
    })
}

fn failure(selected_root: &Path, detail: &str) -> GenerateError {
    internal_generate_error(DiagnosticStage::Emission, detail, selected_root)
}

#[derive(Clone)]
struct EmissionPlan {
    identity: String,
    facade: Ident,
    module_anchor: syn::Path,
    grammar_name: String,
    text_equivalence: TextEquivalence,
    pretty_max_width: usize,
    pretty_indent: usize,
    kind_count: usize,
    lexical_error_kind: u16,
    kind_variants: Vec<Ident>,
    kind_static_texts: Vec<Option<String>>,
    kind_names: Vec<String>,
    atom_names: Vec<String>,
    nodes: Vec<NodeParsePlan>,
    tokens: Vec<TokenParsePlan>,
    token_bindings: Vec<TokenBindingPlan>,
    lexical_rules: Vec<LexRulePlan>,
    word_classifiers: Vec<WordClassifierPlan>,
    word_candidate_kinds: Vec<u16>,
    post_lex: Option<syn::Path>,
    root_configured_traits: Vec<ConfiguredTraitRecipe>,
    private_abi: PrivateAbiPlan,
    capabilities: Vec<FacadeCapability>,
    pretty: PrettyEmissionPlan,
    #[cfg(feature = "diagrams")]
    diagrams: Option<DiagramPlan>,
    explorer: Option<ExtensionEmissionPlan>,
    instrumentation: EmissionInstrumentation,
    plan_families: EmissionPlanFamilies,
}

#[derive(Clone, Debug)]
struct EmissionPlanFamilies {
    configured_traits: u64,
    private_abi: u64,
    capabilities: u64,
    pretty: Option<u64>,
    parse: u64,
    diagrams: Option<u64>,
    explorer: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrivateAbiPlan {
    release: String,
    protocol: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FacadeCapability {
    diagrams: bool,
    explorer: bool,
    arbitrary: bool,
    serde: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrettyEmissionPlan {
    tokens: Vec<Option<PrettyTokenBehavior>>,
    nodes: Vec<Option<PrettyNodeBehavior>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrettyTokenBehavior {
    MatchedOrStoredText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrettyNodeBehavior {
    variants: Vec<Vec<PrettyElementBehavior>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PrettyElementBehavior {
    Fixed {
        spelling: String,
        spacing: PrettySpacing,
    },
    Semantic {
        repeated: bool,
        separator: Option<String>,
        attachment: Option<PrettyAttachmentBehavior>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrettyAttachmentBehavior {
    before: Vec<PrettyAttachedFixedBehavior>,
    after: Vec<PrettyAttachedFixedBehavior>,
    separator: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrettyAttachedFixedBehavior {
    spelling: String,
    spacing: PrettySpacing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtensionEmissionPlan {
    artifacts: Vec<ExtensionArtifactPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtensionArtifactPlan {
    logical_name: String,
    content: String,
}

#[cfg(feature = "diagrams")]
#[derive(Clone, Debug)]
struct DiagramPlan {
    grammar_name: String,
    nodes: Vec<NodeDiagramPlan>,
    fixed_tokens: Vec<(String, String)>,
}

#[cfg(feature = "diagrams")]
#[derive(Clone, Debug)]
struct NodeDiagramPlan {
    semantic_key: String,
    ident: String,
    public_path: Option<String>,
    production: String,
}

#[derive(Clone, Default)]
struct EmissionInstrumentation {
    frozen_dispatch_nodes_consumed: usize,
}

#[derive(Default)]
struct RenderInstrumentation {
    frozen_dispatch_nodes_rendered: usize,
}

#[derive(Clone, Debug)]
struct NodeParsePlan {
    semantic_key: String,
    module: ModuleId,
    ident: Ident,
    visibility: Visibility,
    publicly_materialized: bool,
    loader_anchor: String,
    generics: Generics,
    input_lifetime: syn::Lifetime,
    body: NodeBodyPlan,
    parse_applications: Vec<NodeApplicationParsePlan>,
    capture: PlannedCapturePolicy,
    configured_traits: Vec<ConfiguredTraitRecipe>,
}

#[derive(Clone, Debug)]
struct NodeApplicationParsePlan {
    parse_index: usize,
    ty: Type,
    input_lifetime: syn::Lifetime,
    body: NodeBodyPlan,
}

#[derive(Clone, Debug)]
enum NodeBodyPlan {
    Struct {
        style: ResolvedFieldStyle,
        elements: Vec<ElementParsePlan>,
    },
    Enum {
        variants: Vec<VariantParsePlan>,
        dispatch: PredictiveDispatchPlan,
    },
    Pratt(Box<PrattParsePlan>),
}

#[derive(Clone, Debug)]
struct PrattParsePlan {
    expected_nud_count: usize,
    expected_led_count: usize,
    nud: Vec<PrattNudParsePlan>,
    led: Vec<PrattLedParsePlan>,
    nud_dispatch: PredictiveDispatchPlan,
    led_dispatch: Option<PredictiveDispatchPlan>,
    nud_failure: PlannedFollow,
    nud_selection_follow: ExpectedSet,
    caller_follow_guard: Option<Vec<PrattGuardBranchPlan>>,
}

#[derive(Clone, Debug)]
struct PrattNudParsePlan {
    source_index: usize,
    variant: VariantParsePlan,
    position: PrattPosition,
}

#[derive(Clone, Debug)]
struct PrattLedParsePlan {
    source_index: usize,
    variant: VariantParsePlan,
    lhs: ElementParsePlan,
    position: PrattPosition,
}

#[derive(Clone, Debug)]
struct PrattGuardBranchPlan {
    position: PrattPosition,
    expected: ExpectedSet,
}

#[derive(Clone, Debug)]
struct VariantParsePlan {
    ident: Ident,
    style: ResolvedFieldStyle,
    elements: Vec<ElementParsePlan>,
    capture: Option<PlannedCapturePolicy>,
}

#[derive(Clone, Debug)]
struct PredictiveDispatchPlan {
    root: DispatchTrieNodePlan,
    frozen_sequence_count: usize,
    frozen_node_count: usize,
    frozen_identity: u64,
}

#[derive(Clone, Debug, Default)]
struct DispatchTrieNodePlan {
    target: Option<DispatchTargetPlan>,
    edges: Vec<DispatchTrieEdgePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DispatchTargetPlan {
    variant: usize,
    on_caller_follow: bool,
}

#[derive(Clone, Debug)]
struct DispatchTrieEdgePlan {
    position: DispatchPositionPlan,
    next: DispatchTrieNodePlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DispatchPositionPlan {
    expected: ExpectedSet,
    text_predicate: Option<PlannedTextPredicate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlannedTextPredicate {
    Any,
    Exact(String),
    OneOf(Vec<String>),
    NotIn(Vec<String>),
}

#[derive(Clone, Debug)]
struct ElementParsePlan {
    metadata_name: String,
    kind: ElementParseKind,
}

#[derive(Clone, Debug)]
struct FixedAttachmentParsePlan {
    before: Vec<AttachedFixedParsePlan>,
    semantic: Box<PlannedContainer>,
    after: Vec<AttachedFixedParsePlan>,
}

#[derive(Clone, Debug)]
struct AttachedFixedParsePlan {
    expected: ExpectedSet,
    capture: PlannedCapturePolicy,
    spelling: String,
    spacing: PrettySpacing,
}

#[derive(Clone, Debug)]
enum ElementParseKind {
    Fixed {
        expected: ExpectedSet,
        capture: PlannedCapturePolicy,
        spelling: String,
        spacing: PrettySpacing,
        accessor: Ident,
    },
    Semantic {
        field: FieldPlan,
        terminal: Box<Type>,
        wrappers: Vec<FieldWrapper>,
        container: Box<PlannedContainer>,
        capture: PlannedCapturePolicy,
        child_capture: PlannedCapturePolicy,
        pratt_min_bp: Option<u8>,
        separator: Option<String>,
        attachment: Option<Box<FixedAttachmentParsePlan>>,
    },
}

#[derive(Clone, Debug)]
struct FieldPlan {
    source: ResolvedFieldId,
    ident: Option<Ident>,
    index: usize,
    accessor: Option<Ident>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldWrapper {
    Boxed,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Debug)]
enum PlannedContainer {
    Direct {
        child_follow: PlannedFollow,
        required_failure: PlannedFollow,
    },
    Optional {
        selector: ExpectedSet,
        child_follow: PlannedFollow,
    },
    Repeated {
        operation: ContainerOperation,
        element_selector: ExpectedSet,
        separator_selector: Option<ExpectedSet>,
        optional_trailing: bool,
        stop: PlannedFollow,
        child_follow: PlannedFollow,
        minimum_failure: Option<Box<PlannedFollow>>,
        continuation_failure: Box<PlannedFollow>,
        after_separator_failure: Option<Box<PlannedFollow>>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlannedFollow {
    Caller,
    Static(ExpectedSet),
    StaticAndCaller(ExpectedSet),
}

#[derive(Clone)]
struct TokenParsePlan {
    ident: Ident,
    kind: u16,
    kind_variant: Ident,
    accepted_kind_variants: Vec<Ident>,
    expected: ExpectedSet,
    capture: PlannedCapturePolicy,
    arbitrary_spelling: String,
    configured_traits: Vec<ConfiguredTraitRecipe>,
}

#[derive(Clone)]
struct TokenBindingPlan {
    module: ModuleId,
    semantic_key: String,
    ident: Ident,
    visibility: Visibility,
    publicly_materialized: bool,
    token: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedSet {
    words: Vec<u64>,
    atoms: Vec<u32>,
}

impl ExpectedSet {
    fn empty(kind_count: usize) -> Self {
        Self {
            words: vec![0; kind_count.div_ceil(64)],
            atoms: Vec::new(),
        }
    }

    fn from_kinds(kinds: &KindSet, kind_count: usize) -> Self {
        let mut words = kinds.words().to_vec();
        words.resize(kind_count.div_ceil(64), 0);
        Self {
            words,
            atoms: Vec::new(),
        }
    }

    fn from_positions(
        positions: &[FirstPosition],
        grammar: &crate::GrammarDescription,
        kind_count: usize,
    ) -> Result<Self, String> {
        let mut value = Self::empty(kind_count);
        for position in positions {
            for (left, right) in value.words.iter_mut().zip(position.kinds.words()) {
                *left |= *right;
            }
            for kind in position.kinds.iter() {
                if let Some(atom) = grammar.atoms.iter().position(|atom| {
                    matches!(atom.expression, AtomExpression::Kind(candidate) if candidate == kind)
                }) {
                    value.atoms.push(
                        atom.try_into()
                            .map_err(|_| "generated expectation atom exceeds runtime width")?,
                    );
                }
            }
        }
        value.atoms.sort_unstable();
        value.atoms.dedup();
        Ok(value)
    }

    fn union_assign(&mut self, other: &Self) {
        for (left, right) in self.words.iter_mut().zip(&other.words) {
            *left |= *right;
        }
        self.atoms.extend(other.atoms.iter().copied());
        self.atoms.sort_unstable();
        self.atoms.dedup();
    }
    fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0) && self.atoms.is_empty()
    }
}

impl PrattParsePlan {
    fn validate(&self) -> Result<(), String> {
        if self.nud.len() != self.expected_nud_count || self.nud.is_empty() {
            return Err("the frozen Pratt plan lacks its complete nud dispatch family".to_owned());
        }
        if self.led.len() != self.expected_led_count {
            return Err("the frozen Pratt plan lacks its complete led dispatch family".to_owned());
        }
        if matches!(&self.nud_failure, PlannedFollow::Static(expected) if expected.is_empty()) {
            return Err("the frozen Pratt plan lacks its nud failure family".to_owned());
        }
        validate_predictive_dispatch(&self.nud_dispatch, self.nud.len())?;
        if self.led.is_empty() {
            if self.caller_follow_guard.is_some()
                || self.led_dispatch.is_some()
                || !self.nud_selection_follow.is_empty()
            {
                return Err(
                    "a Pratt plan without led branches has a caller-FOLLOW guard".to_owned(),
                );
            }
            return Ok(());
        }
        let Some(guard) = &self.caller_follow_guard else {
            return Err("the frozen Pratt plan lacks its caller-FOLLOW guard family".to_owned());
        };
        let Some(led_dispatch) = &self.led_dispatch else {
            return Err("the frozen Pratt plan lacks its led dispatch family".to_owned());
        };
        validate_predictive_dispatch(led_dispatch, self.led.len())?;
        if guard.len() != self.led.len() {
            return Err("the frozen Pratt caller-FOLLOW guard is corrupt".to_owned());
        }
        for branch in &self.led {
            let recursive_bp = branch.variant.elements.iter().find_map(|element| {
                let ElementParseKind::Semantic { pratt_min_bp, .. } = &element.kind else {
                    return None;
                };
                *pratt_min_bp
            });
            match (branch.position, recursive_bp) {
                (PrattPosition::Infix { rbp, .. }, Some(actual)) if rbp == actual => {}
                (PrattPosition::Postfix { .. }, None) => {}
                _ => {
                    return Err("the frozen Pratt led binding-power family is corrupt".to_owned());
                }
            }
        }
        if guard.iter().any(|branch| branch.expected.is_empty()) {
            return Err("the frozen Pratt caller-FOLLOW guard is corrupt".to_owned());
        }
        let mut expected_follow = ExpectedSet::empty(self.nud_selection_follow.words.len() * 64);
        for branch in guard {
            expected_follow.union_assign(&branch.expected);
        }
        if expected_follow != self.nud_selection_follow {
            return Err("the frozen Pratt nud FOLLOW family is corrupt".to_owned());
        }
        for branch in &self.nud {
            if let PrattPosition::Prefix { bp } = branch.position {
                let recursive_bp = branch.variant.elements.iter().find_map(|element| {
                    let ElementParseKind::Semantic { pratt_min_bp, .. } = &element.kind else {
                        return None;
                    };
                    *pratt_min_bp
                });
                if recursive_bp != Some(bp) {
                    return Err(
                        "the frozen Pratt prefix binding-power family is corrupt".to_owned()
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedCapturePolicy {
    Inherited,
    Off,
    Optional,
    Mandatory,
}

impl PlannedCapturePolicy {
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::Inherited => "inherited",
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }

    fn render(self, facade: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Inherited => quote!(context.capture_policy()),
            Self::Off => quote!(#facade::__private::CapturePolicy::Off),
            Self::Optional => quote!(#facade::__private::CapturePolicy::Optional),
            Self::Mandatory => quote!(#facade::__private::CapturePolicy::Mandatory),
        }
    }
}

impl From<CaptureMode> for PlannedCapturePolicy {
    fn from(value: CaptureMode) -> Self {
        match value {
            CaptureMode::Off => Self::Off,
            CaptureMode::Optional => Self::Optional,
            CaptureMode::Mandatory => Self::Mandatory,
        }
    }
}

#[derive(Clone)]
struct LexRulePlan {
    pattern: String,
    kind: u16,
    skip: bool,
    callback: Option<syn::Path>,
    fixed: bool,
    priority: u32,
}

#[derive(Clone)]
struct KeywordClassifierPlan {
    canonical_spelling: String,
    kind: u16,
    carriers: Vec<u16>,
    ascii_insensitive: bool,
}

#[derive(Clone)]
struct WordClassifierPlan {
    canonical_spelling: String,
    kind: u16,
    ascii_insensitive: bool,
}

fn fixed_lexical_pattern(spelling: &str, equivalence: TextEquivalence) -> String {
    let word_shaped = is_word_shaped(spelling);
    if equivalence != TextEquivalence::AsciiInsensitive || !word_shaped {
        return regex_syntax::escape(spelling);
    }
    spelling
        .chars()
        .map(|character| {
            if character.is_ascii_alphabetic() {
                format!(
                    "[{}{}]",
                    character.to_ascii_lowercase(),
                    character.to_ascii_uppercase()
                )
            } else {
                regex_syntax::escape(&character.to_string())
            }
        })
        .collect()
}

fn is_word_shaped(spelling: &str) -> bool {
    let mut characters = spelling.chars();
    characters.next().is_some_and(|first| {
        (first == '_' || unicode_ident::is_xid_start(first))
            && characters.all(unicode_ident::is_xid_continue)
    })
}

fn fixed_pretty_spelling(spelling: &str, equivalence: TextEquivalence) -> String {
    if equivalence == TextEquivalence::AsciiInsensitive && is_word_shaped(spelling) {
        spelling.to_ascii_uppercase()
    } else {
        spelling.to_owned()
    }
}

fn lexical_pattern_witness(pattern: &str) -> Result<String, String> {
    use regex_syntax::hir::{Class, Hir, HirKind};

    fn render(hir: &Hir) -> Option<String> {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => Some(String::new()),
            HirKind::Literal(literal) => String::from_utf8(literal.0.to_vec()).ok(),
            HirKind::Class(Class::Unicode(class)) => {
                class.iter().next().map(|range| range.start().to_string())
            }
            HirKind::Class(Class::Bytes(class)) => class.iter().find_map(|range| {
                (range.start()..=range.end())
                    .find(|byte| byte.is_ascii())
                    .map(|byte| char::from(byte).to_string())
            }),
            HirKind::Repetition(repetition) => {
                let item = render(&repetition.sub)?;
                Some(item.repeat(repetition.min.try_into().ok()?))
            }
            HirKind::Capture(capture) => render(&capture.sub),
            HirKind::Concat(parts) => {
                let rendered = parts.iter().map(render).collect::<Option<Vec<_>>>()?;
                Some(rendered.concat())
            }
            HirKind::Alternation(parts) => parts
                .iter()
                .filter_map(render)
                .min_by_key(|candidate| candidate.len()),
        }
    }

    let hir = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|error| {
            format!("validated lexical pattern `{pattern}` no longer parses: {error}")
        })?;
    let witness = render(&hir)
        .ok_or_else(|| format!("lexical pattern `{pattern}` has no UTF-8 synthetic witness"))?;
    if witness.is_empty() {
        return Err(format!(
            "nonempty lexical pattern `{pattern}` produced an empty synthetic witness"
        ));
    }
    Ok(witness)
}

fn publicly_reachable(
    tree: &crate::discovery::DiscoveredModuleTree,
    module: ModuleId,
    visibility: &Visibility,
) -> bool {
    if !is_public(visibility) {
        return false;
    }
    let mut current = module;
    loop {
        let Some(inclusion) = tree.inclusions.iter().find(|entry| entry.id == current) else {
            return false;
        };
        let Some(parent) = inclusion.parent else {
            return true;
        };
        if !is_public(&inclusion.visibility) {
            return false;
        }
        current = parent;
    }
}

impl EmissionPlan {
    fn join(analyzed: &crate::m3::AnalyzedGrammar, cfg: &CargoCfgSnapshot) -> Result<Self, String> {
        let lowered = &analyzed.lowered;
        let resolved = lowered.source_modeled.resolved();
        let output = analyzed
            .analysis
            .output
            .as_ref()
            .ok_or_else(|| "successful M3 output lacks M2 decisions".to_owned())?;
        let decisions = output.decisions();
        let module_anchor = emitted_crate_path(&resolved.build_inputs.module_anchor.segments)?;
        let facade = emitted_ident(&resolved.build_inputs.facade.ident)?;
        let root_configured_traits = resolved
            .configuration
            .syntax
            .derives
            .iter()
            .filter_map(|path| {
                crate::source_model::configured_trait_recipe(
                    path,
                    &resolved.build_inputs.facade.ident,
                )
            })
            .collect::<Vec<_>>();
        let kind_count = lowered.grammar.kind_count + 1;
        let lexical_error_kind = u16::try_from(lowered.grammar.kind_count)
            .map_err(|_| "generated lexical-error kind exceeds runtime width".to_owned())?;
        let kind_variants = planned_kind_variants(lowered)?;
        let kind_static_texts = lowered
            .lexical_plan
            .token_kinds
            .iter()
            .map(|kind| match &kind.identity {
                TokenKindIdentity::Fixed(text) => Some(text.clone()),
                TokenKindIdentity::ContentBase(_) => None,
            })
            .collect::<Vec<_>>();
        let mut kind_names = lowered
            .grammar
            .kinds
            .iter()
            .map(|kind| kind.name.clone())
            .collect::<Vec<_>>();
        kind_names.push("lexical error".to_owned());
        let atom_names = lowered
            .grammar
            .atoms
            .iter()
            .map(|atom| atom.name.clone())
            .collect::<Vec<_>>();

        validate_view_collisions(resolved, cfg)?;

        let mut tokens = Vec::new();
        for (index, content) in resolved.content_tokens.iter().enumerate() {
            let target = lowered
                .lexical_plan
                .parser_targets
                .iter()
                .find(|target| target.semantic == content.id)
                .ok_or_else(|| {
                    format!("content token `{}` lacks a parser target", content.ident)
                })?;
            let content_type = lowered
                .lowering_map
                .content_targets
                .iter()
                .find(|correlation| correlation.semantic == content.id)
                .ok_or_else(|| format!("content token `{}` lacks an M2 type", content.ident))?
                .lowered;
            let TypeShape::Token { atom, .. } = &lowered.grammar.types[content_type.index()].shape
            else {
                return Err(format!(
                    "content token `{}` did not lower to an M2 token type",
                    content.ident
                ));
            };
            let kind = u16::try_from(target.base_kind.index())
                .map_err(|_| "generated token kind exceeds runtime width".to_owned())?;
            let accepted_kinds = target.accepted_kinds();
            let mut expected = ExpectedSet::from_kinds(&accepted_kinds, kind_count);
            expected.atoms.push(atom.index().try_into().map_err(|_| {
                "generated content-token expectation exceeds runtime width".to_owned()
            })?);
            tokens.push(TokenParsePlan {
                ident: format_ident!("__RecursaContentToken{index}"),
                kind,
                kind_variant: kind_variants[target.base_kind.index()].clone(),
                accepted_kind_variants: accepted_kinds
                    .iter()
                    .map(|kind| kind_variants[kind.index()].clone())
                    .collect(),
                expected,
                capture: resolved.build_inputs.spans.into(),
                arbitrary_spelling: lexical_pattern_witness(
                    &lowered
                        .lexical_plan
                        .content_bases
                        .iter()
                        .find(|base| base.kind == target.base_kind)
                        .ok_or_else(|| {
                            format!("content token `{}` lacks a lexical base", content.ident)
                        })?
                        .pattern,
                )?,
                configured_traits: lowered
                    .source_modeled
                    .source_model()
                    .token_definitions
                    .iter()
                    .find_map(|definition| match &definition.kind {
                        TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                            Some(
                                token
                                    .configured_traits
                                    .effective
                                    .iter()
                                    .map(|selection| selection.recipe)
                                    .collect(),
                            )
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        format!(
                            "content token `{}` lacks a configured trait plan",
                            content.ident
                        )
                    })?,
            });
        }
        let token_bindings = resolved
            .token_bindings
            .iter()
            .map(|binding| {
                Ok(TokenBindingPlan {
                    module: binding.module,
                    semantic_key: resolved
                        .tree
                        .inclusions
                        .iter()
                        .find(|inclusion| inclusion.id == binding.module)
                        .map(|inclusion| {
                            inclusion
                                .logical_path
                                .iter()
                                .chain(std::iter::once(&binding.ident))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("::")
                        })
                        .ok_or_else(|| {
                            format!("token binding `{}` has no selected module", binding.ident)
                        })?,
                    ident: emitted_ident(&binding.ident)?,
                    visibility: binding.visibility.clone(),
                    publicly_materialized: publicly_reachable(
                        &resolved.tree,
                        binding.module,
                        &binding.visibility,
                    ),
                    token: binding.target.index(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut nodes = Vec::new();
        let mut instrumentation = EmissionInstrumentation::default();
        for node in &resolved.nodes {
            let applications = lowered
                .applications
                .iter()
                .filter(|application| application.node == node.id)
                .collect::<Vec<_>>();
            if applications.is_empty() {
                return Err(format!(
                    "Node `{}` lacks a materialized application before compound emission",
                    node.ident
                ));
            }
            let configured_traits = lowered
                .source_modeled
                .source_model()
                .nodes
                .iter()
                .find(|source_node| source_node.resolved_node == node.id)
                .ok_or_else(|| format!("Node `{}` lacks a configured trait plan", node.ident))?
                .configured_traits
                .effective
                .iter()
                .map(|selection| selection.recipe)
                .collect();
            let authored_lifetimes = node
                .generics
                .lifetimes()
                .map(|parameter| parameter.lifetime.clone())
                .collect::<Vec<_>>();
            let mut application_plans = Vec::new();
            let mut source_plan = None;
            for application in applications {
                let planned = plan_node(
                    node,
                    application,
                    lowered,
                    decisions,
                    kind_count,
                    &mut instrumentation,
                )?;
                application_plans.push(NodeApplicationParsePlan {
                    parse_index: 0,
                    ty: materialized_application_type(
                        application,
                        &authored_lifetimes,
                        lowered,
                        &planned.input_lifetime,
                    )?,
                    input_lifetime: planned.input_lifetime.clone(),
                    body: planned.body.clone(),
                });
                if source_plan.is_none() {
                    source_plan = Some(planned);
                }
            }
            let mut planned = source_plan
                .ok_or_else(|| format!("Node `{}` lacks an application plan", node.ident))?;
            planned.body = structural_body(node, &application_plans)?;
            planned.parse_applications.append(&mut application_plans);
            planned.configured_traits = configured_traits;
            nodes.push(planned);
        }
        let mut next_parse_index = 0;
        for node in &mut nodes {
            for application in &mut node.parse_applications {
                application.parse_index = next_parse_index;
                next_parse_index += 1;
            }
        }
        let frozen_dispatch_sequences = nodes
            .iter()
            .flat_map(|node| &node.parse_applications)
            .filter_map(|application| match &application.body {
                NodeBodyPlan::Enum { dispatch, .. } => Some(dispatch.frozen_sequence_count),
                NodeBodyPlan::Struct { .. } => None,
                NodeBodyPlan::Pratt(pratt) => Some(
                    pratt.nud_dispatch.frozen_sequence_count
                        + pratt
                            .led_dispatch
                            .as_ref()
                            .map_or(0, |dispatch| dispatch.frozen_sequence_count),
                ),
            })
            .sum::<usize>();
        let frozen_parse_identity = parse_plan_identity(&nodes);
        // M3 has already decided whether each fixed token remains a standalone
        // rule or is recognized through one or more proven carriers. Preserve
        // that decision here instead of reconstructing fixed rules from source.
        let mut lexical_rules: Vec<LexRulePlan> = Vec::new();
        for rule in &lowered.lexical_plan.rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => {
                    fixed_lexical_pattern(text, resolved.build_inputs.text_equivalence)
                }
            };
            let planned = LexRulePlan {
                pattern,
                kind: rule
                    .kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds runtime width".to_owned())?,
                skip: false,
                callback: match &lowered.lexical_plan.token_kinds[rule.kind.index()].identity {
                    TokenKindIdentity::ContentBase(base) => base.callback.clone(),
                    TokenKindIdentity::Fixed(_) => None,
                },
                fixed: matches!(rule.priority, LexicalPriority::Fixed),
                priority: match rule.priority {
                    LexicalPriority::Fixed => 0,
                    LexicalPriority::Pattern { authored } => authored.unwrap_or(0),
                },
            };
            if !lexical_rules.iter().any(|existing| {
                existing.pattern == planned.pattern && existing.kind == planned.kind
            }) {
                lexical_rules.push(planned);
            }
        }
        for rule in &lowered.lexical_plan.non_token_rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => {
                    fixed_lexical_pattern(text, resolved.build_inputs.text_equivalence)
                }
            };
            lexical_rules.push(LexRulePlan {
                pattern,
                kind: 0,
                skip: true,
                callback: None,
                fixed: false,
                priority: 0,
            });
        }
        let keyword_classifiers =
            lowered
                .lexical_plan
                .keywords
                .iter()
                .filter_map(|keyword| {
                    let KeywordRecognition::CombinedCarriers {
                        carrier,
                        additional_carriers,
                    } = &keyword.recognition
                    else {
                        return None;
                    };
                    let carriers = std::iter::once(*carrier)
                        .chain(additional_carriers.iter().copied())
                        .map(|kind| {
                            kind.index().try_into().map_err(|_| {
                                "generated token kind exceeds runtime width".to_owned()
                            })
                        })
                        .collect::<Result<Vec<_>, _>>();
                    Some(carriers.and_then(|carriers| {
                        Ok(KeywordClassifierPlan {
                            canonical_spelling: keyword.canonical_spelling.clone(),
                            kind: keyword.kind.index().try_into().map_err(|_| {
                                "generated token kind exceeds runtime width".to_owned()
                            })?,
                            carriers,
                            ascii_insensitive: resolved.build_inputs.text_equivalence
                                == TextEquivalence::AsciiInsensitive,
                        })
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
        let word_classifiers = lowered
            .lexical_plan
            .keywords
            .iter()
            .map(|keyword| {
                Ok(WordClassifierPlan {
                    canonical_spelling: keyword.canonical_spelling.clone(),
                    kind: keyword
                        .kind
                        .index()
                        .try_into()
                        .map_err(|_| "generated token kind exceeds runtime width".to_owned())?,
                    ascii_insensitive: resolved.build_inputs.text_equivalence
                        == TextEquivalence::AsciiInsensitive,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let word_candidate_kinds = lowered
            .lexical_plan
            .content_bases
            .iter()
            .map(|base| {
                base.kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds runtime width".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?;
        #[cfg(feature = "diagrams")]
        let authoritative_diagrams = resolved
            .build_inputs
            .diagrams
            .then(|| plan_diagrams(lowered, resolved, &nodes, &kind_names, &kind_static_texts))
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagrams = resolved
            .build_inputs
            .diagrams
            .then(|| plan_diagrams(lowered, resolved, &nodes, &kind_names, &kind_static_texts))
            .transpose()?;
        #[cfg(feature = "diagrams")]
        let diagram_enabled = diagrams.is_some();
        #[cfg(not(feature = "diagrams"))]
        let diagram_enabled = false;
        #[cfg(feature = "diagrams")]
        let frozen_diagrams = authoritative_diagrams.as_ref().map(plan_family_identity);
        #[cfg(not(feature = "diagrams"))]
        let frozen_diagrams = None;
        let private_abi = emitted_private_abi_plan();
        let explorer = None;
        let pretty = emitted_pretty_plan(&nodes, &tokens);
        let capabilities =
            emitted_capability_plan(&nodes, &tokens, diagram_enabled, explorer.is_some());
        let authoritative_explorer = authoritative_explorer_plan(resolved);
        let plan_families = EmissionPlanFamilies {
            configured_traits: authoritative_configured_trait_identity(lowered, resolved),
            private_abi: plan_family_identity(&authoritative_private_abi_plan()),
            capabilities: plan_family_identity(&authoritative_capability_plan(
                lowered,
                resolved.build_inputs.diagrams,
                authoritative_explorer.is_some(),
            )),
            pretty: authoritative_pretty_plan(lowered, resolved)?
                .as_ref()
                .map(plan_family_identity),
            parse: frozen_parse_identity,
            diagrams: frozen_diagrams,
            explorer: authoritative_explorer.as_ref().map(plan_family_identity),
        };

        let mut canonical = format!(
            "{EMITTER_IDENTITY};release={};protocol={PRIVATE_PROTOCOL};facade={facade};anchor={};kinds={kind_count};edition={:?};cfg={:?};",
            env!("CARGO_PKG_VERSION"),
            resolved.build_inputs.module_anchor.segments.join("::"),
            resolved.build_inputs.discovery.edition,
            resolved.build_inputs.discovery.cfg_snapshot,
        );
        for source in &resolved.tree.files {
            canonical.push_str(&format!(
                ";source={}:{}:{:016x}",
                source.id.index(),
                source.path.display(),
                stable_hash(source.text.as_bytes())
            ));
        }
        for node in &nodes {
            canonical.push_str(&format!(
                ";node={}:{}:{}:{}",
                node.semantic_key,
                node.loader_anchor,
                node.ident,
                node.generics.to_token_stream()
            ));
        }
        for token in &tokens {
            canonical.push_str(&format!(
                ";token={}:{}:{}:{:?}",
                token.ident,
                token.kind,
                token.capture.canonical_name(),
                token.accepted_kind_variants,
            ));
        }
        for binding in &token_bindings {
            canonical.push_str(&format!(
                ";binding={}:{}:{}:{}",
                binding.module.index(),
                binding.ident,
                binding.visibility.to_token_stream(),
                binding.token
            ));
        }
        for rule in &lexical_rules {
            canonical.push_str(&format!(
                ";rule={:?}:{}:{}:{}:{}:{}",
                rule.pattern,
                rule.kind,
                rule.skip,
                rule.callback
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), crate::source_syntax::path_text,),
                rule.fixed,
                rule.priority,
            ));
        }
        for classifier in &keyword_classifiers {
            canonical.push_str(&format!(
                ";classify={:?}:{}:{:?}:{}",
                classifier.canonical_spelling,
                classifier.kind,
                classifier.carriers,
                classifier.ascii_insensitive,
            ));
        }
        for classifier in &word_classifiers {
            canonical.push_str(&format!(
                ";classify_word={:?}:{}:{}",
                classifier.canonical_spelling, classifier.kind, classifier.ascii_insensitive,
            ));
        }
        canonical.push_str(&format!(";word_candidates={word_candidate_kinds:?}"));
        #[cfg(feature = "diagrams")]
        canonical.push_str(if diagrams.is_some() {
            ";capabilities=core,diagrams"
        } else {
            ";capabilities=core"
        });
        #[cfg(not(feature = "diagrams"))]
        canonical.push_str(";capabilities=core");
        canonical.push_str(&format!(
            ";frozen_dispatch_sequences={frozen_dispatch_sequences};frozen_dispatch_nodes={};emitter_dispatch_decisions=0;frozen_parse_identity={frozen_parse_identity:016x}",
            instrumentation.frozen_dispatch_nodes_consumed,
        ));
        if let Some(post_lex) = &lowered.source_modeled.source_model().post_lex {
            canonical.push_str(&format!(
                ";post_lex={}",
                crate::source_syntax::path_text(post_lex),
            ));
        }
        let identity = format!("g_{:016x}", stable_hash(canonical.as_bytes()));
        let plan = Self {
            identity,
            facade,
            module_anchor,
            grammar_name: lowered.grammar.name.clone(),
            text_equivalence: lowered.grammar.text_equivalence,
            pretty_max_width: resolved.build_inputs.pretty.max_width,
            pretty_indent: resolved.build_inputs.pretty.indent,
            kind_count,
            lexical_error_kind,
            kind_variants,
            kind_static_texts,
            kind_names,
            atom_names,
            nodes,
            tokens,
            token_bindings,
            lexical_rules,
            word_classifiers,
            word_candidate_kinds,
            post_lex: lowered.source_modeled.source_model().post_lex.clone(),
            root_configured_traits,
            private_abi,
            capabilities,
            pretty,
            #[cfg(feature = "diagrams")]
            diagrams,
            explorer,
            instrumentation,
            plan_families,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<(), String> {
        if self.identity.is_empty() || self.nodes.is_empty() {
            return Err("the Emission plan lacks a Generation identity or Nodes".to_owned());
        }
        if self.kind_variants.len() + 1 != self.kind_count
            || self.kind_static_texts.len() + 1 != self.kind_count
            || self.kind_names.len() != self.kind_count
        {
            return Err("the generated token-kind plan has inconsistent width".to_owned());
        }
        if self.lexical_rules.is_empty() {
            return Err("the generated lexical plan has no rules".to_owned());
        }
        #[cfg(feature = "diagrams")]
        if self
            .diagrams
            .as_ref()
            .is_some_and(|diagrams| diagrams.nodes.len() != self.nodes.len())
        {
            return Err("the diagram plan does not cover every frozen Node".to_owned());
        }
        self.validate_plan_families()?;
        let mut frozen_nodes = 0;
        for node in &self.nodes {
            for application in &node.parse_applications {
                match &application.body {
                    NodeBodyPlan::Enum { variants, dispatch } => {
                        if dispatch.frozen_sequence_count == 0 {
                            return Err(
                                "the emitted predictive plan has no frozen sequences".to_owned()
                            );
                        }
                        frozen_nodes += validate_predictive_dispatch(dispatch, variants.len())?;
                    }
                    NodeBodyPlan::Pratt(pratt) => {
                        pratt.validate()?;
                        frozen_nodes += pratt.nud_dispatch.frozen_node_count;
                        frozen_nodes += pratt
                            .led_dispatch
                            .as_ref()
                            .map_or(0, |dispatch| dispatch.frozen_node_count);
                    }
                    NodeBodyPlan::Struct { .. } => {}
                }
            }
        }
        if self.instrumentation.frozen_dispatch_nodes_consumed != frozen_nodes
            || self.plan_families.parse != parse_plan_identity(&self.nodes)
        {
            return Err("the emitted predictive plan diverges from frozen M2 decisions".to_owned());
        }
        Ok(())
    }

    fn validate_plan_families(&self) -> Result<(), String> {
        if self.plan_families.configured_traits
            != configured_trait_plan_identity(&self.nodes, &self.tokens)
        {
            return Err(
                "the configured-trait plan family diverges from its frozen identity".to_owned(),
            );
        }
        if self.plan_families.private_abi != plan_family_identity(&self.private_abi) {
            return Err("the private ABI plan family diverges from its frozen identity".to_owned());
        }
        let pretty = pretty_family_identity(&self.pretty);
        if self.plan_families.pretty != pretty {
            return Err("the Pretty plan family diverges from its frozen identity".to_owned());
        }
        #[cfg(feature = "diagrams")]
        if self.plan_families.diagrams != self.diagrams.as_ref().map(plan_family_identity) {
            return Err("the diagram plan family diverges from its frozen identity".to_owned());
        }
        #[cfg(not(feature = "diagrams"))]
        if self.plan_families.diagrams.is_some() {
            return Err("the diagram plan family diverges from its frozen identity".to_owned());
        }
        if self.plan_families.explorer != self.explorer.as_ref().map(plan_family_identity) {
            return Err("the explorer plan family diverges from its frozen identity".to_owned());
        }
        if self.plan_families.capabilities != plan_family_identity(&self.capabilities) {
            return Err("the capability plan family diverges from its frozen identity".to_owned());
        }
        Ok(())
    }
}

#[cfg(feature = "diagrams")]
fn plan_diagrams(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
    nodes: &[NodeParsePlan],
    kind_names: &[String],
    kind_static_texts: &[Option<String>],
) -> Result<DiagramPlan, String> {
    let nodes = nodes
        .iter()
        .map(|node| {
            let source = resolved
                .nodes
                .iter()
                .find(|source| source.key.segments.join("::") == node.semantic_key)
                .ok_or_else(|| {
                    format!(
                        "diagram plan cannot correlate frozen Node `{}`",
                        node.semantic_key
                    )
                })?;
            let public_path = node_is_publicly_linkable(resolved, source)
                .then(|| format!("crate::{}", node.semantic_key));
            let application = lowered
                .applications
                .iter()
                .find(|application| application.node == source.id)
                .ok_or_else(|| {
                    format!(
                        "diagram plan cannot find the materialized application for `{}`",
                        node.semantic_key
                    )
                })?;
            Ok(NodeDiagramPlan {
                semantic_key: node.semantic_key.clone(),
                ident: node.ident.unraw().to_string(),
                public_path,
                production: diagram_production(node, application, lowered)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let fixed_tokens = kind_names
        .iter()
        .zip(kind_static_texts)
        .filter_map(|(name, spelling)| {
            spelling
                .as_ref()
                .map(|spelling| (name.clone(), spelling.clone()))
        })
        .collect();
    Ok(DiagramPlan {
        grammar_name: resolved.build_inputs.module_anchor.segments.join("::"),
        nodes,
        fixed_tokens,
    })
}

#[cfg(feature = "diagrams")]
fn node_is_publicly_linkable(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    node: &ResolvedNode,
) -> bool {
    if !matches!(node.visibility, Visibility::Public(_)) {
        return false;
    }
    let mut module = Some(node.module);
    while let Some(current) = module {
        let inclusion = &resolved.tree.inclusions[current.index()];
        if inclusion.parent.is_some() && !matches!(inclusion.visibility, Visibility::Public(_)) {
            return false;
        }
        module = inclusion.parent;
    }
    true
}

#[cfg(feature = "diagrams")]
fn diagram_production(
    node: &NodeParsePlan,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let right = match (&node.body, &application.body) {
        (
            NodeBodyPlan::Struct { elements, .. },
            MaterializedNodeBody::Struct(materialized)
            | MaterializedNodeBody::Presence(materialized),
        ) => diagram_sequence(elements, materialized, lowered)?,
        (
            NodeBodyPlan::Enum { variants, .. },
            MaterializedNodeBody::Enum(materialized_variants),
        ) => {
            if variants.len() != materialized_variants.len() {
                return Err("a diagram enum lost a frozen materialized variant".to_owned());
            }
            let alternatives = variants
                .iter()
                .zip(materialized_variants)
                .map(|(variant, materialized)| {
                    let sequence =
                        diagram_sequence(&variant.elements, &materialized.elements, lowered)?;
                    Ok(if sequence == "ε" {
                        variant.ident.unraw().to_string()
                    } else {
                        format!("{}({sequence})", variant.ident.unraw())
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            alternatives.join(" | ")
        }
        (NodeBodyPlan::Pratt(pratt), MaterializedNodeBody::Pratt(materialized)) => {
            diagram_pratt_production(pratt, materialized, lowered)?
        }
        _ => {
            return Err(format!(
                "Node `{}` diagram and materialized shapes disagree",
                node.semantic_key
            ));
        }
    };
    Ok(format!("{} ::= {right}", node.ident.unraw()))
}

#[cfg(feature = "diagrams")]
fn diagram_pratt_production(
    pratt: &PrattParsePlan,
    materialized: &[MaterializedPrattVariant],
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    if pratt.expected_nud_count + pratt.expected_led_count != materialized.len() {
        return Err("a Pratt diagram lost a frozen materialized variant".to_owned());
    }
    let mut alternatives = Vec::with_capacity(materialized.len());
    for (source_index, materialized) in materialized.iter().enumerate() {
        let (position, variant, sequence) = match materialized {
            MaterializedPrattVariant::Nud(materialized) => {
                let branch = unique_pratt_nud(pratt, source_index)?;
                validate_pratt_position(Some(branch.position), materialized)?;
                validate_diagram_variant_name(&branch.variant, &materialized.common.name)?;
                let sequence =
                    diagram_sequence(&branch.variant.elements, &materialized.body, lowered)?;
                (branch.position, &branch.variant, sequence)
            }
            MaterializedPrattVariant::Led(materialized) => {
                let branch = unique_pratt_led(pratt, source_index)?;
                validate_pratt_led_position(Some(branch.position), materialized)?;
                validate_diagram_variant_name(&branch.variant, &materialized.common.name)?;
                let lhs = diagram_pratt_lhs(&branch.lhs, materialized, lowered)?;
                let tail = diagram_sequence(&branch.variant.elements, &materialized.tail, lowered)?;
                let sequence = if tail == "ε" {
                    lhs
                } else {
                    format!("{lhs} {tail}")
                };
                (branch.position, &branch.variant, sequence)
            }
        };
        let role = diagram_pratt_role(position);
        let name = variant.ident.unraw();
        alternatives.push(if sequence == "ε" {
            format!("{role} {name}")
        } else {
            format!("{role} {name}({sequence})")
        });
    }
    Ok(alternatives.join(" | "))
}

#[cfg(feature = "diagrams")]
fn unique_pratt_nud(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattNudParsePlan, String> {
    let mut matching = pratt
        .nud
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "a Pratt diagram lacks its frozen nud branch".to_owned())?;
    if matching.next().is_some() {
        return Err("a Pratt diagram has duplicate frozen nud branches".to_owned());
    }
    Ok(branch)
}

#[cfg(feature = "diagrams")]
fn unique_pratt_led(
    pratt: &PrattParsePlan,
    source_index: usize,
) -> Result<&PrattLedParsePlan, String> {
    let mut matching = pratt
        .led
        .iter()
        .filter(|branch| branch.source_index == source_index);
    let branch = matching
        .next()
        .ok_or_else(|| "a Pratt diagram lacks its frozen led branch".to_owned())?;
    if matching.next().is_some() {
        return Err("a Pratt diagram has duplicate frozen led branches".to_owned());
    }
    Ok(branch)
}

#[cfg(feature = "diagrams")]
fn validate_diagram_variant_name(
    variant: &VariantParsePlan,
    materialized_name: &str,
) -> Result<(), String> {
    if variant.ident.unraw() == materialized_name {
        Ok(())
    } else {
        Err("a Pratt diagram variant name diverges from materialized source".to_owned())
    }
}

#[cfg(feature = "diagrams")]
fn diagram_pratt_lhs(
    lhs: &ElementParsePlan,
    materialized: &MaterializedPrattLed,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let ElementParseKind::Semantic {
        container,
        pratt_min_bp,
        ..
    } = &lhs.kind
    else {
        return Err("a Pratt diagram lhs is not a frozen semantic element".to_owned());
    };
    if pratt_min_bp.is_some() {
        return Err("a Pratt diagram lhs unexpectedly has a minimum binding power".to_owned());
    }
    let terminal =
        diagram_terminal_name(&ClosedType::Node(materialized.lhs.owner.clone()), lowered)?;
    diagram_semantic_element(&terminal, None, container)
}

#[cfg(feature = "diagrams")]
fn diagram_pratt_role(position: PrattPosition) -> String {
    match position {
        PrattPosition::Atom => "atom".to_owned(),
        PrattPosition::Prefix { bp } => format!("prefix(bp={bp})"),
        PrattPosition::Infix { lbp, rbp } => format!("infix(lbp={lbp}, rbp={rbp})"),
        PrattPosition::Postfix { bp } => format!("postfix(bp={bp})"),
    }
}

#[cfg(feature = "diagrams")]
fn diagram_sequence(
    elements: &[ElementParsePlan],
    materialized: &[MaterializedElement],
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    if elements.len() != materialized.len() {
        return Err("a diagram sequence lost a frozen materialized element".to_owned());
    }
    if elements.is_empty() {
        return Ok("ε".to_owned());
    }
    elements
        .iter()
        .zip(materialized)
        .map(
            |(element, materialized)| match (&element.kind, materialized) {
                (
                    ElementParseKind::Fixed { spelling, .. },
                    MaterializedElement::FixedToken { .. },
                ) => Ok(format!("{spelling:?}")),
                (
                    ElementParseKind::Semantic {
                        container,
                        attachment: planned_attachment,
                        ..
                    },
                    MaterializedElement::SemanticField {
                        ty,
                        attachment,
                        separator,
                        ..
                    },
                ) => {
                    if planned_attachment.is_some() != attachment.is_some() {
                        return Err(
                            "a diagram attachment diverges from the frozen parse plan".to_owned()
                        );
                    }
                    let terminal = diagram_terminal_name(ty, lowered)?;
                    let terminal = attachment.as_ref().map_or_else(
                        || Ok(terminal.clone()),
                        |attachment| diagram_attachment_unit(&terminal, attachment, lowered),
                    )?;
                    let separator = diagram_separator_spelling(separator.as_ref(), lowered)?;
                    diagram_semantic_element(&terminal, separator.as_deref(), container)
                }
                _ => Err("a diagram element and its frozen materialization disagree".to_owned()),
            },
        )
        .collect::<Result<Vec<_>, String>>()
        .map(|elements| elements.join(" "))
}

#[cfg(feature = "diagrams")]
fn diagram_attachment_unit(
    terminal: &str,
    attachment: &MaterializedFixedAttachment,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    let source = lowered.source_modeled.source_model();
    let spelling = |token_use: crate::lowering::TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[usage.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("a diagram attachment is not fixed syntax".to_owned());
        };
        Ok(format!("{:?}", fixed.spelling))
    };
    attachment
        .before
        .iter()
        .map(|token_use| spelling(*token_use))
        .chain(std::iter::once(Ok(terminal.to_owned())))
        .chain(
            attachment
                .after
                .iter()
                .map(|token_use| spelling(*token_use)),
        )
        .collect::<Result<Vec<_>, _>>()
        .map(|elements| elements.join(" "))
}

#[cfg(feature = "diagrams")]
fn diagram_semantic_element(
    terminal: &str,
    separator: Option<&str>,
    container: &PlannedContainer,
) -> Result<String, String> {
    match container {
        PlannedContainer::Direct { .. } => {
            if separator.is_some() {
                return Err("a direct diagram element unexpectedly has a separator".to_owned());
            }
            Ok(terminal.to_owned())
        }
        PlannedContainer::Optional { .. } => {
            if separator.is_some() {
                return Err("an optional diagram element unexpectedly has a separator".to_owned());
            }
            Ok(format!("Option<{terminal}>"))
        }
        PlannedContainer::Repeated {
            operation,
            separator_selector,
            optional_trailing,
            ..
        } => {
            if separator.is_some() != separator_selector.is_some() {
                return Err(
                    "a diagram separator diverges from the frozen container machine".to_owned(),
                );
            }
            if *optional_trailing && separator.is_none() {
                return Err("a diagram has optional trailing syntax without a separator".to_owned());
            }
            let mut rendered = match operation {
                ContainerOperation::ZeroOrMore => format!("Vec<{terminal}>"),
                ContainerOperation::OneOrMore => format!("Vec1<{terminal}>"),
                ContainerOperation::OptionalOneOrMore => {
                    format!("Option<Vec1<{terminal}>>")
                }
                ContainerOperation::Direct | ContainerOperation::Optional => {
                    return Err("a repeated diagram element has a scalar operation".to_owned());
                }
            };
            if let Some(separator) = separator {
                rendered.push_str(&format!(" separated by {separator:?}"));
                if *optional_trailing {
                    rendered.push_str(&format!(" (optional trailing {separator:?})"));
                }
            }
            Ok(rendered)
        }
    }
}

fn parse_plan_identity(nodes: &[NodeParsePlan]) -> u64 {
    stable_hash(format!("{nodes:?}").as_bytes())
}

fn plan_family_identity(value: &impl std::fmt::Debug) -> u64 {
    stable_hash(format!("{value:?}").as_bytes())
}

fn emitted_private_abi_plan() -> PrivateAbiPlan {
    PrivateAbiPlan {
        release: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PRIVATE_PROTOCOL,
    }
}

fn authoritative_private_abi_plan() -> PrivateAbiPlan {
    // This contract belongs to the facade/codegen release, independently of
    // the mutable emission plan which later renders the handshake.
    PrivateAbiPlan {
        release: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PRIVATE_PROTOCOL,
    }
}

fn authoritative_configured_trait_identity(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
) -> u64 {
    let source = lowered.source_modeled.source_model();
    let mut canonical = String::new();
    for (index, resolved_node) in resolved.nodes.iter().enumerate() {
        canonical.push_str(&format!("node-{index}:"));
        if let Some(node) = source
            .nodes
            .iter()
            .find(|node| node.resolved_node == resolved_node.id)
        {
            for selection in &node.configured_traits.effective {
                canonical.push_str(&format!("{:?},", selection.recipe));
            }
        } else {
            canonical.push_str("<missing>");
        }
        canonical.push(';');
    }
    for (index, content) in resolved.content_tokens.iter().enumerate() {
        canonical.push_str(&format!("token-{index}:"));
        if let Some(token) =
            source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                        Some(token)
                    }
                    TokenDefinitionKind::Content(_)
                    | TokenDefinitionKind::Fixed(_)
                    | TokenDefinitionKind::LexicalBase(_) => None,
                })
        {
            for selection in &token.configured_traits.effective {
                canonical.push_str(&format!("{:?},", selection.recipe));
            }
        } else {
            canonical.push_str("<missing>");
        }
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
}

fn configured_trait_plan_identity(nodes: &[NodeParsePlan], tokens: &[TokenParsePlan]) -> u64 {
    let mut canonical = String::new();
    for (index, node) in nodes.iter().enumerate() {
        canonical.push_str(&format!("node-{index}:"));
        for recipe in &node.configured_traits {
            canonical.push_str(&format!("{recipe:?},"));
        }
        canonical.push(';');
    }
    for (index, token) in tokens.iter().enumerate() {
        canonical.push_str(&format!("token-{index}:"));
        for recipe in &token.configured_traits {
            canonical.push_str(&format!("{recipe:?},"));
        }
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
}

fn accepted_capabilities(
    requested_arbitrary: bool,
    requested_serde: bool,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let mut accepted = Vec::new();
    for arbitrary in [false, true] {
        for serde in [false, true] {
            if (!requested_arbitrary || arbitrary) && (!requested_serde || serde) {
                accepted.push(FacadeCapability {
                    diagrams,
                    explorer,
                    arbitrary,
                    serde,
                });
            }
        }
    }
    accepted
}

fn emitted_capability_plan(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let recipes = nodes
        .iter()
        .flat_map(|node| &node.configured_traits)
        .chain(tokens.iter().flat_map(|token| &token.configured_traits));
    capability_plan_from_recipes(recipes.copied(), diagrams, explorer)
}

fn authoritative_capability_plan(
    lowered: &crate::materialization::LoweredGrammar,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let source = lowered.source_modeled.source_model();
    let recipes = source
        .nodes
        .iter()
        .flat_map(|node| &node.configured_traits.effective)
        .chain(
            source
                .token_definitions
                .iter()
                .flat_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) => {
                        token.configured_traits.effective.as_slice()
                    }
                    TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => &[],
                }),
        )
        .map(|selection| selection.recipe);
    capability_plan_from_recipes(recipes, diagrams, explorer)
}

fn capability_plan_from_recipes(
    recipes: impl Iterator<Item = ConfiguredTraitRecipe>,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let mut requested_arbitrary = false;
    let mut requested_serde = false;
    for recipe in recipes {
        requested_arbitrary |= recipe == ConfiguredTraitRecipe::Arbitrary;
        requested_serde |= matches!(
            recipe,
            ConfiguredTraitRecipe::Serialize | ConfiguredTraitRecipe::Deserialize
        );
    }
    accepted_capabilities(requested_arbitrary, requested_serde, diagrams, explorer)
}

fn emitted_pretty_plan(nodes: &[NodeParsePlan], tokens: &[TokenParsePlan]) -> PrettyEmissionPlan {
    PrettyEmissionPlan {
        tokens: tokens
            .iter()
            .map(|token| {
                token
                    .configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then_some(PrettyTokenBehavior::MatchedOrStoredText)
            })
            .collect(),
        nodes: nodes
            .iter()
            .map(|node| {
                node.configured_traits
                    .contains(&ConfiguredTraitRecipe::Pretty)
                    .then(|| emitted_pretty_node_behavior(node))
            })
            .collect(),
    }
}

fn emitted_pretty_node_behavior(node: &NodeParsePlan) -> PrettyNodeBehavior {
    let variants = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => vec![emitted_pretty_elements(elements)],
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
        NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .iter()
            .map(|variant| emitted_pretty_elements(&variant.elements))
            .collect(),
    };
    PrettyNodeBehavior { variants }
}

fn emitted_pretty_elements(elements: &[ElementParsePlan]) -> Vec<PrettyElementBehavior> {
    elements
        .iter()
        .map(|element| match &element.kind {
            ElementParseKind::Fixed {
                spelling, spacing, ..
            } => PrettyElementBehavior::Fixed {
                spelling: spelling.clone(),
                spacing: *spacing,
            },
            ElementParseKind::Semantic {
                container,
                attachment,
                separator,
                ..
            } => PrettyElementBehavior::Semantic {
                repeated: matches!(container.as_ref(), PlannedContainer::Repeated { .. }),
                separator: separator.clone(),
                attachment: attachment
                    .as_ref()
                    .map(|attachment| PrettyAttachmentBehavior {
                        before: attachment
                            .before
                            .iter()
                            .map(|fixed| PrettyAttachedFixedBehavior {
                                spelling: fixed.spelling.clone(),
                                spacing: fixed.spacing,
                            })
                            .collect(),
                        after: attachment
                            .after
                            .iter()
                            .map(|fixed| PrettyAttachedFixedBehavior {
                                spelling: fixed.spelling.clone(),
                                spacing: fixed.spacing,
                            })
                            .collect(),
                        separator: separator.clone(),
                    }),
            },
        })
        .collect()
}

fn authoritative_pretty_plan(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
) -> Result<Option<PrettyEmissionPlan>, String> {
    let source = lowered.source_modeled.source_model();
    let tokens = resolved
        .content_tokens
        .iter()
        .map(|content| {
            source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                        Some(token)
                    }
                    TokenDefinitionKind::Content(_)
                    | TokenDefinitionKind::Fixed(_)
                    | TokenDefinitionKind::LexicalBase(_) => None,
                })
                .is_some_and(|token| {
                    token
                        .configured_traits
                        .effective
                        .iter()
                        .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
                })
                .then_some(PrettyTokenBehavior::MatchedOrStoredText)
        })
        .collect::<Vec<_>>();
    let nodes = resolved
        .nodes
        .iter()
        .map(|resolved_node| {
            let source_node = source
                .nodes
                .iter()
                .find(|node| node.resolved_node == resolved_node.id)
                .ok_or_else(|| {
                    format!(
                        "Node `{}` lacks an authoritative Pretty plan",
                        resolved_node.ident
                    )
                })?;
            if !source_node
                .configured_traits
                .effective
                .iter()
                .any(|selection| selection.recipe == ConfiguredTraitRecipe::Pretty)
            {
                return Ok(None);
            }
            let application = lowered
                .applications
                .iter()
                .find(|application| application.node == resolved_node.id)
                .ok_or_else(|| {
                    format!(
                        "Node `{}` lacks an authoritative application",
                        resolved_node.ident
                    )
                })?;
            authoritative_pretty_node_behavior(lowered, application).map(Some)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let plan = PrettyEmissionPlan { tokens, nodes };
    Ok(pretty_family_identity(&plan).map(|_| plan))
}

fn authoritative_pretty_node_behavior(
    lowered: &crate::materialization::LoweredGrammar,
    application: &MaterializedNodeApplication,
) -> Result<PrettyNodeBehavior, String> {
    let variants = match &application.body {
        MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements) => {
            vec![authoritative_pretty_elements(lowered, elements)?]
        }
        MaterializedNodeBody::Enum(variants) => variants
            .iter()
            .map(|variant| authoritative_pretty_elements(lowered, &variant.elements))
            .collect::<Result<Vec<_>, String>>()?,
        MaterializedNodeBody::Pratt(variants) => variants
            .iter()
            .map(|variant| match variant {
                MaterializedPrattVariant::Nud(nud) => {
                    authoritative_pretty_elements(lowered, &nud.body)
                }
                MaterializedPrattVariant::Led(led) => {
                    let mut elements = vec![PrettyElementBehavior::Semantic {
                        repeated: false,
                        separator: None,
                        attachment: None,
                    }];
                    elements.extend(authoritative_pretty_elements(lowered, &led.tail)?);
                    Ok(elements)
                }
            })
            .collect::<Result<Vec<_>, String>>()?,
    };
    Ok(PrettyNodeBehavior { variants })
}

fn authoritative_pretty_elements(
    lowered: &crate::materialization::LoweredGrammar,
    elements: &[MaterializedElement],
) -> Result<Vec<PrettyElementBehavior>, String> {
    let source = lowered.source_modeled.source_model();
    elements
        .iter()
        .map(|element| match element {
            MaterializedElement::FixedToken { token_use } => {
                let usage = &source.token_uses[token_use.index()];
                let definition = &source.token_definitions[usage.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err(
                        "a materialized fixed Pretty element is not a fixed token".to_owned()
                    );
                };
                Ok(PrettyElementBehavior::Fixed {
                    spelling: fixed_pretty_spelling(
                        &fixed.spelling,
                        lowered.grammar.text_equivalence,
                    ),
                    spacing: fixed.pretty_spacing,
                })
            }
            MaterializedElement::SemanticField {
                ty,
                attachment,
                separator,
                ..
            } => {
                let repeated = field_wrappers(ty)?.iter().any(|wrapper| {
                    matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
                });
                Ok(PrettyElementBehavior::Semantic {
                    repeated,
                    separator: authoritative_pretty_separator(lowered, separator.as_ref())?,
                    attachment: attachment
                        .as_ref()
                        .map(|attachment| {
                            authoritative_pretty_attachment(lowered, attachment, separator.as_ref())
                        })
                        .transpose()?,
                })
            }
        })
        .collect()
}

fn authoritative_pretty_separator(
    lowered: &crate::materialization::LoweredGrammar,
    separator: Option<&crate::source_model::Separator>,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source = lowered.source_modeled.source_model();
    let usage = &source.token_uses[separator.token_use.index()];
    let definition = &source.token_definitions[usage.definition.index()];
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("a Pretty separator is not fixed syntax".to_owned());
    };
    Ok(Some(fixed_pretty_spelling(
        &fixed.spelling,
        lowered.grammar.text_equivalence,
    )))
}

fn authoritative_pretty_attachment(
    lowered: &crate::materialization::LoweredGrammar,
    attachment: &MaterializedFixedAttachment,
    separator: Option<&crate::source_model::Separator>,
) -> Result<PrettyAttachmentBehavior, String> {
    let source = lowered.source_modeled.source_model();
    let fixed = |token_use: crate::lowering::TokenUseId| {
        let usage = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[usage.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("an attached Pretty element is not fixed syntax".to_owned());
        };
        Ok(PrettyAttachedFixedBehavior {
            spelling: fixed_pretty_spelling(&fixed.spelling, lowered.grammar.text_equivalence),
            spacing: fixed.pretty_spacing,
        })
    };
    let separator = authoritative_pretty_separator(lowered, separator)?;
    Ok(PrettyAttachmentBehavior {
        before: attachment
            .before
            .iter()
            .map(|token_use| fixed(*token_use))
            .collect::<Result<_, _>>()?,
        after: attachment
            .after
            .iter()
            .map(|token_use| fixed(*token_use))
            .collect::<Result<_, _>>()?,
        separator,
    })
}

fn pretty_family_identity(plan: &PrettyEmissionPlan) -> Option<u64> {
    (plan.tokens.iter().any(Option::is_some) || plan.nodes.iter().any(Option::is_some))
        .then(|| plan_family_identity(plan))
}

fn authoritative_explorer_plan(
    _resolved: &crate::resolution::ResolvedSourceGrammar,
) -> Option<ExtensionEmissionPlan> {
    // The current resolved grammar has no explorer request. Ticket #70 extends
    // this authoritative seam with its resolved explorer plan.
    None
}

fn planned_kind_variants(
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Vec<Ident>, String> {
    let mut used = BTreeSet::new();
    let mut variants = Vec::new();
    for kind in &lowered.lexical_plan.token_kinds {
        let candidate = match &kind.identity {
            TokenKindIdentity::Fixed(_) => lowered
                .lexical_plan
                .lowering_map
                .token_definitions
                .iter()
                .find(|correlation| correlation.lowered == kind.id)
                .and_then(|correlation| {
                    lowered
                        .source_modeled
                        .source_model()
                        .token_definitions
                        .get(correlation.source.index())
                })
                .map_or_else(
                    || format!("Fixed{}", kind.id.index()),
                    |definition| definition.name.clone(),
                ),
            TokenKindIdentity::ContentBase(_) => lowered
                .lexical_plan
                .parser_targets
                .iter()
                .find(|target| target.base_kind == kind.id)
                .map_or_else(
                    || format!("Content{}", kind.id.index()),
                    |target| target.name.clone(),
                ),
        };
        let mut candidate = candidate;
        if !used.insert(candidate.clone()) {
            candidate.push_str(&format!("Kind{}", kind.id.index()));
            used.insert(candidate.clone());
        }
        variants.push(emitted_ident(&candidate)?);
    }
    Ok(variants)
}

fn validate_view_collisions(
    resolved: &crate::resolution::ResolvedSourceGrammar,
    cfg: &CargoCfgSnapshot,
) -> Result<(), String> {
    let mut occupied = BTreeMap::<(ModuleId, String), &'static str>::new();
    for inclusion in &resolved.tree.inclusions {
        for name in inclusion.items.iter().filter_map(|item| {
            crate::source_syntax::configure_attributes(
                crate::source_syntax::item_attributes(item),
                cfg,
            )
            .ok()
            .flatten()
            .and_then(|_| item_ident(item))
        }) {
            occupied.insert((inclusion.id, name), "an authored item");
        }
    }
    for import in &resolved.imports {
        if matches!(
            import.target,
            Some(crate::resolution::ImportTarget::External)
        ) {
            continue;
        }
        if let Some(name) = &import.local_name {
            occupied.insert((import.module, name.clone()), "an authored import");
        }
    }
    for node in &resolved.nodes {
        occupied.insert((node.module, node.ident.clone()), "an authored Node");
    }
    for binding in &resolved.token_bindings {
        occupied.insert(
            (binding.module, binding.ident.clone()),
            "a generated content-token binding",
        );
    }

    for node in &resolved.nodes {
        let view_name = format!("{}View", node.ident);
        if let Some(origin) = occupied.get(&(node.module, view_name.clone())) {
            return Err(format!(
                "generated concrete view `{view_name}` collides with {origin} beside Node `{}`",
                node.ident
            ));
        }
        occupied.insert((node.module, view_name), "another generated concrete view");
    }
    for binding in &resolved.token_bindings {
        let view_name = format!("{}View", binding.ident);
        if let Some(origin) = occupied.get(&(binding.module, view_name.clone())) {
            return Err(format!(
                "generated concrete view `{view_name}` collides with {origin} beside content token `{}`",
                binding.ident
            ));
        }
        occupied.insert(
            (binding.module, view_name),
            "another generated concrete view",
        );
    }
    Ok(())
}

fn item_ident(item: &syn::Item) -> Option<String> {
    let ident = match item {
        syn::Item::Enum(item) => &item.ident,
        syn::Item::ExternCrate(item) => &item.ident,
        syn::Item::Mod(item) => &item.ident,
        syn::Item::Struct(item) => &item.ident,
        syn::Item::Trait(item) => &item.ident,
        syn::Item::TraitAlias(item) => &item.ident,
        syn::Item::Type(item) => &item.ident,
        syn::Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.unraw().to_string())
}

fn planned_dispatch_position(
    position: &FirstPosition,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<DispatchPositionPlan, String> {
    let text_predicate = position
        .text_predicate
        .map(|predicate| {
            grammar
                .text_predicates
                .get(predicate.index())
                .ok_or_else(|| "frozen dispatch references an unknown text predicate".to_owned())
                .map(|predicate| match predicate {
                    TextPredicate::Any => PlannedTextPredicate::Any,
                    TextPredicate::Exact(text) => PlannedTextPredicate::Exact(text.clone()),
                    TextPredicate::OneOf(options) => PlannedTextPredicate::OneOf(options.clone()),
                    TextPredicate::NotIn(options) => PlannedTextPredicate::NotIn(options.clone()),
                })
        })
        .transpose()?;
    Ok(DispatchPositionPlan {
        expected: ExpectedSet::from_positions(std::slice::from_ref(position), grammar, kind_count)?,
        text_predicate,
    })
}

fn plan_predictive_dispatch_node(
    frozen: &crate::DispatchTrieNode,
    variants: &[(crate::DeclId, usize)],
    grammar: &crate::GrammarDescription,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<DispatchTrieNodePlan, String> {
    instrumentation.frozen_dispatch_nodes_consumed += 1;
    let target = frozen
        .terminal()
        .map(|terminal| {
            variants
                .iter()
                .find(|(declaration, _)| *declaration == terminal.declaration())
                .map(|(_, variant)| DispatchTargetPlan {
                    variant: *variant,
                    on_caller_follow: terminal.on_caller_follow(),
                })
                .ok_or_else(|| "frozen dispatch terminal lacks a source variant".to_owned())
        })
        .transpose()?;
    let edges = frozen
        .edges()
        .iter()
        .map(|edge| {
            Ok(DispatchTrieEdgePlan {
                position: planned_dispatch_position(edge.position(), grammar, kind_count)?,
                next: plan_predictive_dispatch_node(
                    edge.next(),
                    variants,
                    grammar,
                    kind_count,
                    instrumentation,
                )?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DispatchTrieNodePlan { target, edges })
}

fn predictive_dispatch_identity(root: &DispatchTrieNodePlan) -> u64 {
    stable_hash(format!("{root:?}").as_bytes())
}

fn validate_predictive_dispatch(
    dispatch: &PredictiveDispatchPlan,
    variants: usize,
) -> Result<usize, String> {
    fn validate_node(node: &DispatchTrieNodePlan, variants: usize) -> Result<usize, String> {
        if node.target.is_none() && node.edges.is_empty() {
            return Err("frozen predictive dispatch contains an empty node".to_owned());
        }
        if node.target.is_some_and(|target| target.variant >= variants) {
            return Err("frozen predictive dispatch references an unknown variant".to_owned());
        }
        if node
            .target
            .is_some_and(|target| !target.on_caller_follow && !node.edges.is_empty())
        {
            return Err("frozen predictive dispatch extends an unconditional terminal".to_owned());
        }
        if node.edges.iter().enumerate().any(|(index, edge)| {
            node.edges[index + 1..]
                .iter()
                .any(|other| other.position == edge.position)
        }) {
            return Err("frozen predictive dispatch contains duplicate edges".to_owned());
        }
        let mut count = 1;
        for edge in &node.edges {
            count += validate_node(&edge.next, variants)?;
        }
        Ok(count)
    }

    let count = validate_node(&dispatch.root, variants)?;
    if count != dispatch.frozen_node_count
        || predictive_dispatch_identity(&dispatch.root) != dispatch.frozen_identity
    {
        return Err("the joined predictive plan diverges from its frozen identity".to_owned());
    }
    Ok(count)
}

fn plan_node(
    node: &ResolvedNode,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<NodeParsePlan, String> {
    let inherited_capture = node
        .capture
        .unwrap_or(lowered.source_modeled.resolved().build_inputs.spans);
    let application_type = lowered
        .lowering_map
        .applications
        .iter()
        .find(|correlation| correlation.application == application.id)
        .ok_or_else(|| format!("Node `{}` lacks an M2 type correlation", node.ident))?
        .lowered;
    let source = &lowered.source_modeled.resolved().tree.files[node.source.index()];
    let physical_source = normalized_source_identity(&source.path)?;
    let (line, column) = source_line_column(&source.text, node.range.start)?;
    let loader_anchor = declaration_loader_anchor(&physical_source, line, column, &node.ident);
    let input_lifetime = node
        .generics
        .params
        .iter()
        .find_map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => Some(parameter.lifetime.clone()),
            GenericParam::Type(_) | GenericParam::Const(_) => None,
        })
        .or_else(|| application_input_lifetime(application))
        .unwrap_or_else(|| syn::parse_quote!('__recursa_input));

    let body = match (
        &application.body,
        &node.body,
        &lowered.grammar.types[application_type.index()].shape,
    ) {
        (
            MaterializedNodeBody::Struct(elements) | MaterializedNodeBody::Presence(elements),
            ResolvedNodeBody::Struct { style },
            TypeShape::Sequence { sites },
        ) => NodeBodyPlan::Struct {
            style: *style,
            elements: plan_elements(
                node,
                None,
                elements,
                sites,
                lowered,
                decisions,
                kind_count,
                &input_lifetime,
                None,
            )?,
        },
        (
            MaterializedNodeBody::Enum(variants),
            ResolvedNodeBody::Enum {
                variants: resolved_variants,
            },
            TypeShape::Enum { alternatives },
        ) => {
            let dispatch = decisions
                .dispatches()
                .iter()
                .find(|dispatch| {
                    dispatch.owner() == application_type && dispatch.kind() == DispatchKind::Enum
                })
                .ok_or_else(|| format!("enum Node `{}` lacks its dispatch plan", node.ident))?;
            let failure_expected = decisions
                .failure_edges()
                .iter()
                .find(|edge| {
                    edge.owner() == application_type
                        && edge.site().is_none()
                        && edge.kind() == FailureEdgeKind::EnumDispatch
                })
                .ok_or_else(|| {
                    format!("enum Node `{}` lacks its frozen failure edge", node.ident)
                })?;
            let (failure_expected, failure_uses_caller) = match failure_expected.expected() {
                ExpectedRecipe::Static(first) => (first.as_slice(), false),
                ExpectedRecipe::Follow(follow) => (follow.static_first(), follow.uses_caller()),
                _ => {
                    return Err(format!(
                        "enum Node `{}` has an invalid dispatch failure recipe",
                        node.ident
                    ));
                }
            };
            let failure_expected =
                ExpectedSet::from_positions(failure_expected, &lowered.grammar, kind_count)?;
            let mut planned = Vec::new();
            let mut variants_by_declaration = Vec::new();
            for variant in variants {
                let resolved_variant = resolved_variants
                    .iter()
                    .find(|candidate| candidate.id == variant.resolved_variant)
                    .ok_or_else(|| "materialized enum variant lacks source shape".to_owned())?;
                let correlation = lowered
                    .lowering_map
                    .variants
                    .iter()
                    .find(|correlation| {
                        correlation.application == application.id
                            && correlation.variant == variant.resolved_variant
                    })
                    .ok_or_else(|| "materialized enum variant lacks M2 correlation".to_owned())?;
                let branch = dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized enum variant lacks dispatch branch".to_owned())?;
                if branch.sequences().is_empty() {
                    return Err(format!(
                        "enum variant `{}` lacks a frozen dispatch sequence",
                        variant.name
                    ));
                }
                variants_by_declaration.push((branch.declaration(), planned.len()));
                let alternative = alternatives
                    .iter()
                    .find(|alternative| alternative.declaration == correlation.lowered)
                    .ok_or_else(|| {
                        "enum dispatch branch lacks frozen alternative sites".to_owned()
                    })?;
                planned.push(VariantParsePlan {
                    ident: emitted_ident(&variant.name)?,
                    style: resolved_variant.style,
                    elements: plan_elements(
                        node,
                        Some(variant),
                        &variant.elements,
                        &alternative.sites,
                        lowered,
                        decisions,
                        kind_count,
                        &input_lifetime,
                        None,
                    )?,
                    capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                });
            }
            let frozen_branch_count = dispatch.branches().len();
            let frozen_sequence_count = dispatch
                .branches()
                .iter()
                .map(|branch| branch.sequences().len())
                .sum();
            let nodes_before = instrumentation.frozen_dispatch_nodes_consumed;
            let root = plan_predictive_dispatch_node(
                dispatch.trie(),
                &variants_by_declaration,
                &lowered.grammar,
                kind_count,
                instrumentation,
            )?;
            let frozen_node_count = instrumentation.frozen_dispatch_nodes_consumed - nodes_before;
            let dispatch = PredictiveDispatchPlan {
                frozen_identity: predictive_dispatch_identity(&root),
                root,
                frozen_sequence_count,
                frozen_node_count,
            };
            let mut root_expected = ExpectedSet::empty(kind_count);
            for edge in &dispatch.root.edges {
                root_expected.union_assign(&edge.position.expected);
            }
            if root_expected != failure_expected {
                return Err(format!(
                    "enum Node `{}` dispatch and failure products disagree",
                    node.ident
                ));
            }
            let root_uses_caller = dispatch
                .root
                .target
                .is_some_and(|target| target.on_caller_follow);
            if root_uses_caller != failure_uses_caller {
                return Err(format!(
                    "enum Node `{}` dispatch and caller-FOLLOW failure products disagree",
                    node.ident
                ));
            }
            if variants_by_declaration.len() != frozen_branch_count
                || dispatch.frozen_sequence_count == 0
            {
                return Err(format!(
                    "enum Node `{}` lost a frozen dispatch sequence",
                    node.ident
                ));
            }
            NodeBodyPlan::Enum {
                variants: planned,
                dispatch,
            }
        }
        (
            MaterializedNodeBody::Pratt(materialized),
            ResolvedNodeBody::Enum { variants: resolved },
            TypeShape::Pratt { variants: semantic },
        ) => NodeBodyPlan::Pratt(Box::new(plan_pratt_node(
            node,
            application,
            application_type,
            materialized,
            resolved,
            semantic,
            lowered,
            decisions,
            kind_count,
            &input_lifetime,
            instrumentation,
        )?)),
        _ => {
            return Err(format!(
                "Node `{}` source shape and frozen M2 shape disagree",
                node.ident
            ));
        }
    };
    Ok(NodeParsePlan {
        semantic_key: node.key.segments.join("::"),
        module: node.module,
        ident: emitted_ident(&node.ident)?,
        visibility: node.visibility.clone(),
        publicly_materialized: publicly_reachable(
            &lowered.source_modeled.resolved().tree,
            node.module,
            &node.visibility,
        ),
        loader_anchor,
        generics: node.generics.clone(),
        input_lifetime,
        body,
        parse_applications: Vec::new(),
        capture: inherited_capture.into(),
        configured_traits: Vec::new(),
    })
}

fn structural_body(
    node: &ResolvedNode,
    applications: &[NodeApplicationParsePlan],
) -> Result<NodeBodyPlan, String> {
    let mut body = applications
        .first()
        .ok_or_else(|| format!("Node `{}` lacks an application plan", node.ident))?
        .body
        .clone();
    let mut terminals = BTreeMap::<usize, BTreeSet<String>>::new();
    for application in applications {
        for_each_element(&application.body, &mut |element| {
            if let ElementParseKind::Semantic {
                field, terminal, ..
            } = &element.kind
            {
                terminals
                    .entry(field.source.index())
                    .or_default()
                    .insert(terminal.to_token_stream().to_string());
            }
        });
    }
    for_each_element_mut(&mut body, &mut |element| {
        let ElementParseKind::Semantic {
            field,
            terminal,
            wrappers,
            ..
        } = &mut element.kind
        else {
            return Ok(());
        };
        let source = node
            .fields
            .iter()
            .find(|candidate| candidate.id == field.source)
            .ok_or_else(|| "planned field has no authored source field".to_owned())?;
        if terminals
            .get(&field.source.index())
            .is_some_and(|types| types.len() > 1)
            || resolved_type_uses_generic_parameter(&source.ty)
        {
            **terminal = authored_terminal_type(&source.syntax.ty, wrappers)?;
        }
        Ok(())
    })?;
    disambiguate_fixed_accessors(&mut body)?;
    validate_accessor_names(node, &body)?;
    Ok(body)
}

fn resolved_type_uses_generic_parameter(ty: &ResolvedType) -> bool {
    match &ty.kind {
        ResolvedTypeKind::Semantic {
            target, arguments, ..
        } => {
            matches!(target, SemanticTarget::GenericParameter(_))
                || arguments.iter().any(|argument| match argument {
                    ResolvedGenericArgument::Type(ty) => resolved_type_uses_generic_parameter(ty),
                    ResolvedGenericArgument::Lifetime(_)
                    | ResolvedGenericArgument::Unsupported(_) => false,
                })
        }
        ResolvedTypeKind::Group(inner)
        | ResolvedTypeKind::Parenthesized(inner)
        | ResolvedTypeKind::Reference(inner) => resolved_type_uses_generic_parameter(inner),
        ResolvedTypeKind::Tuple(elements) => {
            elements.iter().any(resolved_type_uses_generic_parameter)
        }
        ResolvedTypeKind::Unsupported => false,
    }
}

fn for_each_element(body: &NodeBodyPlan, visit: &mut impl FnMut(&ElementParsePlan)) {
    match body {
        NodeBodyPlan::Struct { elements, .. } => elements.iter().for_each(visit),
        NodeBodyPlan::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| &variant.elements)
            .for_each(visit),
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &pratt.nud {
                branch.variant.elements.iter().for_each(&mut *visit);
            }
            for branch in &pratt.led {
                visit(&branch.lhs);
                branch.variant.elements.iter().for_each(&mut *visit);
            }
        }
    }
}

fn for_each_element_mut(
    body: &mut NodeBodyPlan,
    visit: &mut impl FnMut(&mut ElementParsePlan) -> Result<(), String>,
) -> Result<(), String> {
    match body {
        NodeBodyPlan::Struct { elements, .. } => {
            for element in elements {
                visit(element)?;
            }
        }
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                for element in &mut variant.elements {
                    visit(element)?;
                }
            }
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &mut pratt.nud {
                for element in &mut branch.variant.elements {
                    visit(element)?;
                }
            }
            for branch in &mut pratt.led {
                visit(&mut branch.lhs)?;
                for element in &mut branch.variant.elements {
                    visit(element)?;
                }
            }
        }
    }
    Ok(())
}

fn authored_terminal_type(authored: &Type, wrappers: &[FieldWrapper]) -> Result<Type, String> {
    fn ungroup(ty: &Type) -> &Type {
        match ty {
            Type::Group(group) => ungroup(&group.elem),
            Type::Paren(paren) => ungroup(&paren.elem),
            _ => ty,
        }
    }

    let mut terminal = authored.clone();
    for wrapper in wrappers {
        let expected = match wrapper {
            FieldWrapper::Boxed => "Box",
            FieldWrapper::Optional => "Option",
            FieldWrapper::ZeroOrMore => "Vec",
            FieldWrapper::OneOrMore => "Vec1",
        };
        let Type::Path(path) = ungroup(&terminal) else {
            return Err(format!(
                "authored generic field wrapper `{expected}` is not a type path"
            ));
        };
        let segment = path
            .path
            .segments
            .last()
            .ok_or_else(|| "authored generic field type path is empty".to_owned())?;
        if segment.ident.unraw() != expected {
            return Err(format!(
                "authored generic field expected `{expected}`, found `{}`",
                segment.ident
            ));
        }
        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return Err(format!("authored `{expected}` field has no type argument"));
        };
        terminal = arguments
            .args
            .iter()
            .find_map(|argument| match argument {
                GenericArgument::Type(ty) => Some(ty.clone()),
                _ => None,
            })
            .ok_or_else(|| format!("authored `{expected}` field has no type argument"))?;
    }
    Ok(terminal)
}

fn disambiguate_fixed_accessors(body: &mut NodeBodyPlan) -> Result<(), String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for_each_element(body, &mut |element| {
        let name = match &element.kind {
            ElementParseKind::Fixed { accessor, .. } => Some(accessor.unraw().to_string()),
            ElementParseKind::Semantic { field, .. } => field
                .accessor
                .as_ref()
                .map(|accessor| accessor.unraw().to_string()),
        };
        if let Some(name) = name {
            *counts.entry(name).or_default() += 1;
        }
    });
    let mut occupied = counts
        .iter()
        .filter_map(|(name, count)| (*count == 1).then_some(name.clone()))
        .collect::<BTreeSet<_>>();

    fn qualify(
        elements: &mut [ElementParsePlan],
        variant: Option<&Ident>,
        counts: &BTreeMap<String, usize>,
        occupied: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for (index, element) in elements.iter_mut().enumerate() {
            let ElementParseKind::Fixed { accessor, .. } = &mut element.kind else {
                continue;
            };
            let original = accessor.unraw().to_string();
            if counts.get(&original).copied().unwrap_or(0) <= 1 {
                continue;
            }
            let prefix = variant.map_or_else(
                || "fixed".to_owned(),
                |variant| snake_case(&variant.unraw().to_string()),
            );
            let mut candidate = format!("{prefix}_{original}");
            if occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{original}");
            }
            let mut suffix = 2;
            while occupied.contains(&candidate) {
                candidate = format!("{prefix}_element_{index}_{suffix}_{original}");
                suffix += 1;
            }
            *accessor = emitted_ident(&candidate)?;
            occupied.insert(candidate);
        }
        Ok(())
    }

    match body {
        NodeBodyPlan::Struct { elements, .. } => qualify(elements, None, &counts, &mut occupied)?,
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                qualify(
                    &mut variant.elements,
                    Some(&variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &mut pratt.nud {
                qualify(
                    &mut branch.variant.elements,
                    Some(&branch.variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
            for branch in &mut pratt.led {
                qualify(
                    &mut branch.variant.elements,
                    Some(&branch.variant.ident),
                    &counts,
                    &mut occupied,
                )?;
            }
        }
    }
    Ok(())
}

fn application_input_lifetime(application: &MaterializedNodeApplication) -> Option<syn::Lifetime> {
    fn first(closed: &ClosedType) -> Option<syn::Lifetime> {
        match closed {
            ClosedType::Node(node) => node.lifetimes.first().cloned(),
            ClosedType::Boxed(inner)
            | ClosedType::Optional(inner)
            | ClosedType::ZeroOrMore(inner)
            | ClosedType::OneOrMore(inner) => first(inner),
            ClosedType::ContentToken(_) => None,
        }
    }

    application.type_arguments.iter().find_map(first)
}

#[allow(clippy::too_many_arguments)]
fn plan_pratt_node(
    node: &ResolvedNode,
    application: &MaterializedNodeApplication,
    application_type: crate::TypeId,
    materialized: &[MaterializedPrattVariant],
    resolved_variants: &[crate::resolution::ResolvedNodeVariant],
    semantic_variants: &[crate::PrattVariant],
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    input_lifetime: &syn::Lifetime,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PrattParsePlan, String> {
    let nud_dispatch = decisions
        .dispatches()
        .iter()
        .find(|dispatch| {
            dispatch.owner() == application_type && dispatch.kind() == DispatchKind::PrattNud
        })
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud dispatch plan", node.ident))?;
    let led_dispatch = decisions.dispatches().iter().find(|dispatch| {
        dispatch.owner() == application_type && dispatch.kind() == DispatchKind::PrattLed
    });

    let nud_failure = decisions
        .failure_edges()
        .iter()
        .find(|edge| {
            edge.owner() == application_type && edge.kind() == FailureEdgeKind::PrattNudDispatch
        })
        .ok_or_else(|| format!("Pratt Node `{}` lacks its nud failure edge", node.ident))?;
    let nud_failure =
        planned_expected_recipe(nud_failure.expected(), &lowered.grammar, kind_count)?;
    let guard = decisions.failure_edges().iter().find(|edge| {
        edge.owner() == application_type && edge.kind() == FailureEdgeKind::PrattCallerFollowGuard
    });
    let guard_branches = match guard.map(|edge| edge.expected()) {
        Some(ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches }) => Some(branches),
        Some(_) => return Err("a Pratt caller-FOLLOW guard has the wrong recipe".to_owned()),
        None => None,
    };

    let mut nud = Vec::new();
    let mut led = Vec::new();
    let mut nud_by_declaration = Vec::new();
    let mut led_by_declaration = Vec::new();
    for (source_index, variant) in materialized.iter().enumerate() {
        let common = match variant {
            MaterializedPrattVariant::Nud(nud) => &nud.common,
            MaterializedPrattVariant::Led(led) => &led.common,
        };
        let resolved = resolved_variants
            .iter()
            .find(|candidate| candidate.id == common.resolved_variant)
            .ok_or_else(|| "materialized Pratt variant lacks source shape".to_owned())?;
        let correlation = lowered
            .lowering_map
            .variants
            .iter()
            .find(|correlation| {
                correlation.application == application.id
                    && correlation.variant == common.resolved_variant
            })
            .ok_or_else(|| "materialized Pratt variant lacks M2 correlation".to_owned())?;
        let semantic = semantic_variants
            .iter()
            .find(|variant| variant.declaration == correlation.lowered)
            .ok_or_else(|| "materialized Pratt variant lacks frozen semantic sites".to_owned())?;
        match variant {
            MaterializedPrattVariant::Nud(materialized_nud) => {
                let branch = nud_dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized Pratt nud lacks dispatch branch".to_owned())?;
                validate_pratt_position(branch.position(), materialized_nud)?;
                let position = branch
                    .position()
                    .ok_or_else(|| "a frozen Pratt nud branch lacks a position".to_owned())?;
                let operand = match position {
                    PrattPosition::Prefix { bp } => Some((application_type, bp)),
                    PrattPosition::Atom => None,
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. } => {
                        return Err("a frozen nud branch has a led position".to_owned());
                    }
                };
                nud_by_declaration.push((branch.declaration(), nud.len()));
                nud.push(PrattNudParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements: plan_elements(
                            node,
                            Some(&MaterializedVariant {
                                resolved_variant: common.resolved_variant,
                                name: common.name.clone(),
                                elements: materialized_nud.body.clone(),
                            }),
                            &materialized_nud.body,
                            &semantic.sites,
                            lowered,
                            decisions,
                            kind_count,
                            input_lifetime,
                            operand,
                        )?,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                    },
                    position,
                });
            }
            MaterializedPrattVariant::Led(materialized_led) => {
                let dispatch = led_dispatch.ok_or_else(|| {
                    format!("Pratt Node `{}` lacks its led dispatch plan", node.ident)
                })?;
                let branch = dispatch
                    .branches()
                    .iter()
                    .find(|branch| branch.declaration() == correlation.lowered)
                    .ok_or_else(|| "materialized Pratt led lacks dispatch branch".to_owned())?;
                validate_pratt_led_position(branch.position(), materialized_led)?;
                let position = branch
                    .position()
                    .ok_or_else(|| "a frozen Pratt led branch lacks a position".to_owned())?;
                let operand = match position {
                    PrattPosition::Infix { rbp, .. } => Some((application_type, rbp)),
                    PrattPosition::Postfix { .. } => None,
                    PrattPosition::Atom | PrattPosition::Prefix { .. } => {
                        return Err("a frozen led branch has a nud position".to_owned());
                    }
                };
                let synthetic_variant = MaterializedVariant {
                    resolved_variant: common.resolved_variant,
                    name: common.name.clone(),
                    elements: materialized_led.tail.clone(),
                };
                led_by_declaration.push((branch.declaration(), led.len()));
                led.push(PrattLedParsePlan {
                    source_index,
                    variant: VariantParsePlan {
                        ident: emitted_ident(&common.name)?,
                        style: resolved.style,
                        elements: plan_elements(
                            node,
                            Some(&synthetic_variant),
                            &materialized_led.tail,
                            &semantic.sites,
                            lowered,
                            decisions,
                            kind_count,
                            input_lifetime,
                            operand,
                        )?,
                        capture: resolved.capture.map(PlannedCapturePolicy::from),
                    },
                    lhs: plan_pratt_lhs(node, resolved, materialized_led, lowered, input_lifetime)?,
                    position,
                });
            }
        }
    }

    let nud_dispatch = plan_joined_dispatch(
        nud_dispatch,
        &nud_by_declaration,
        lowered,
        kind_count,
        instrumentation,
    )?;
    let led_dispatch = led_dispatch
        .map(|dispatch| {
            plan_joined_dispatch(
                dispatch,
                &led_by_declaration,
                lowered,
                kind_count,
                instrumentation,
            )
        })
        .transpose()?;
    let mut nud_dispatch_expected = ExpectedSet::empty(kind_count);
    for edge in &nud_dispatch.root.edges {
        nud_dispatch_expected.union_assign(&edge.position.expected);
    }
    let (nud_failure_expected, nud_failure_uses_caller) = match &nud_failure {
        PlannedFollow::Caller => (ExpectedSet::empty(kind_count), true),
        PlannedFollow::Static(expected) => (expected.clone(), false),
        PlannedFollow::StaticAndCaller(expected) => (expected.clone(), true),
    };
    if nud_dispatch_expected != nud_failure_expected
        || nud_dispatch
            .root
            .target
            .is_some_and(|target| target.on_caller_follow)
            != nud_failure_uses_caller
    {
        return Err("the frozen Pratt nud dispatch and failure products disagree".to_owned());
    }
    let caller_follow_guard = match (guard_branches, &led_dispatch) {
        (Some(branches), Some(dispatch)) => {
            let frozen_led = decisions
                .dispatches()
                .iter()
                .find(|candidate| {
                    candidate.owner() == application_type
                        && candidate.kind() == DispatchKind::PrattLed
                })
                .expect("the joined led dispatch came from frozen decisions");
            if branches.as_slice() != frozen_led.branches() {
                return Err("the frozen Pratt guard and led dispatch branches disagree".to_owned());
            }
            let _ = dispatch;
            Some(
                branches
                    .iter()
                    .map(|branch| {
                        let position = branch.position().ok_or_else(|| {
                            "a frozen Pratt guard branch lacks a position".to_owned()
                        })?;
                        let first = branch
                            .sequences()
                            .iter()
                            .filter_map(|sequence| sequence.positions().first().cloned())
                            .collect::<Vec<_>>();
                        let expected =
                            ExpectedSet::from_positions(&first, &lowered.grammar, kind_count)?;
                        if expected.is_empty() {
                            return Err("a frozen Pratt guard branch is empty".to_owned());
                        }
                        Ok(PrattGuardBranchPlan { position, expected })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )
        }
        (None, None) => None,
        _ => return Err("the frozen Pratt led dispatch and guard family disagree".to_owned()),
    };

    let mut nud_selection_follow = ExpectedSet::empty(kind_count);
    for branch in caller_follow_guard.iter().flatten() {
        nud_selection_follow.union_assign(&branch.expected);
    }

    Ok(PrattParsePlan {
        expected_nud_count: semantic_variants
            .iter()
            .filter(|variant| {
                matches!(
                    variant.position,
                    PrattPosition::Atom | PrattPosition::Prefix { .. }
                )
            })
            .count(),
        expected_led_count: semantic_variants
            .iter()
            .filter(|variant| {
                matches!(
                    variant.position,
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
                )
            })
            .count(),
        nud,
        led,
        nud_dispatch,
        led_dispatch,
        nud_failure,
        nud_selection_follow,
        caller_follow_guard,
    })
}

fn plan_joined_dispatch(
    dispatch: &crate::DispatchPlan,
    variants: &[(crate::DeclId, usize)],
    lowered: &crate::materialization::LoweredGrammar,
    kind_count: usize,
    instrumentation: &mut EmissionInstrumentation,
) -> Result<PredictiveDispatchPlan, String> {
    let nodes_before = instrumentation.frozen_dispatch_nodes_consumed;
    let root = plan_predictive_dispatch_node(
        dispatch.trie(),
        variants,
        &lowered.grammar,
        kind_count,
        instrumentation,
    )?;
    Ok(PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(&root),
        root,
        frozen_sequence_count: dispatch
            .branches()
            .iter()
            .map(|branch| branch.sequences().len())
            .sum(),
        frozen_node_count: instrumentation.frozen_dispatch_nodes_consumed - nodes_before,
    })
}

fn validate_pratt_position(
    position: Option<PrattPosition>,
    variant: &MaterializedPrattNud,
) -> Result<(), String> {
    let expected = match variant.position {
        MaterializedPrattNudPosition::Atom => PrattPosition::Atom,
        MaterializedPrattNudPosition::Prefix { bp } => PrattPosition::Prefix { bp },
    };
    if position == Some(expected) {
        Ok(())
    } else {
        Err("the frozen Pratt nud position disagrees with materialized source".to_owned())
    }
}

fn validate_pratt_led_position(
    position: Option<PrattPosition>,
    variant: &MaterializedPrattLed,
) -> Result<(), String> {
    let expected = match variant.position {
        MaterializedPrattLedPosition::Infix { lbp, rbp } => PrattPosition::Infix { lbp, rbp },
        MaterializedPrattLedPosition::Postfix { bp } => PrattPosition::Postfix { bp },
    };
    if position == Some(expected) {
        Ok(())
    } else {
        Err("the frozen Pratt led position disagrees with materialized source".to_owned())
    }
}

fn plan_pratt_lhs(
    node: &ResolvedNode,
    resolved_variant: &crate::resolution::ResolvedNodeVariant,
    led: &MaterializedPrattLed,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<ElementParsePlan, String> {
    let resolved_field = node
        .fields
        .iter()
        .find(|candidate| candidate.id == led.lhs.field)
        .ok_or_else(|| "materialized Pratt lhs lacks source syntax".to_owned())?;
    let field_index = resolved_variant
        .fields
        .iter()
        .position(|candidate| *candidate == led.lhs.field)
        .ok_or_else(|| "materialized Pratt lhs is absent from its source variant".to_owned())?;
    let base = resolved_field
        .syntax
        .ident
        .clone()
        .unwrap_or_else(|| format_ident!("field_{field_index}"));
    let variant_name = snake_case(&resolved_variant.ident);
    let accessor = if base.unraw() == variant_name {
        base.clone()
    } else {
        format_ident!("{}_{}", variant_name, base)
    };
    let variant_capture = resolved_variant
        .capture
        .map(PlannedCapturePolicy::from)
        .or_else(|| node.capture.map(PlannedCapturePolicy::from))
        .unwrap_or(PlannedCapturePolicy::Inherited);
    let field_override = resolved_field.capture.map(PlannedCapturePolicy::from);
    let closed = ClosedType::Boxed(Box::new(ClosedType::Node(led.lhs.owner.clone())));
    Ok(ElementParsePlan {
        metadata_name: format!("{}_{}", variant_name, base.unraw()),
        kind: ElementParseKind::Semantic {
            field: FieldPlan {
                source: resolved_field.id,
                ident: resolved_field.syntax.ident.clone(),
                index: field_index,
                accessor: Some(accessor),
            },
            terminal: Box::new(terminal_type(&closed, lowered, input_lifetime)?),
            wrappers: vec![FieldWrapper::Boxed],
            container: Box::new(PlannedContainer::Direct {
                child_follow: PlannedFollow::Caller,
                required_failure: PlannedFollow::Caller,
            }),
            capture: field_override.unwrap_or(variant_capture),
            child_capture: field_override.unwrap_or(variant_capture),
            pratt_min_bp: None,
            separator: None,
            attachment: None,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn plan_elements(
    node: &ResolvedNode,
    variant: Option<&MaterializedVariant>,
    elements: &[MaterializedElement],
    sites: &[SiteId],
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    input_lifetime: &syn::Lifetime,
    pratt_operand: Option<(crate::TypeId, u8)>,
) -> Result<Vec<ElementParsePlan>, String> {
    if elements.len() != sites.len() {
        return Err(format!(
            "Node `{}` has mismatched source elements and M2 parse sites",
            node.ident
        ));
    }
    let node_capture = node
        .capture
        .map(PlannedCapturePolicy::from)
        .unwrap_or(PlannedCapturePolicy::Inherited);
    let variant_override = variant
        .and_then(|variant| match &node.body {
            ResolvedNodeBody::Enum { variants } => variants
                .iter()
                .find(|candidate| candidate.id == variant.resolved_variant)
                .and_then(|variant| variant.capture),
            ResolvedNodeBody::Struct { .. } => None,
        })
        .map(PlannedCapturePolicy::from);
    let variant_capture = variant_override.unwrap_or(node_capture);
    let mut semantic_index = 0;
    elements
        .iter()
        .zip(sites)
        .map(|(element, site)| match element {
            MaterializedElement::FixedToken { token_use } => {
                let source_model = lowered.source_modeled.source_model();
                let token_use = &source_model.token_uses[token_use.index()];
                let definition = &source_model.token_definitions[token_use.definition.index()];
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return Err("a fixed element did not reference fixed-token syntax".to_owned());
                };
                let PlannedFollow::Static(expected) = planned_failure_edge(
                    decisions,
                    *site,
                    FailureEdgeKind::RequiredSite,
                    &lowered.grammar,
                    kind_count,
                )?
                else {
                    return Err("a fixed element has a dynamic failure recipe".to_owned());
                };
                let name = fixed_element_name(node, token_use, definition, fixed);
                let accessor = fixed_accessor_ident(&name)?;
                let capture = fixed_element_field(node, token_use)
                    .and_then(|field| field.capture)
                    .map(PlannedCapturePolicy::from)
                    .unwrap_or(variant_capture);
                Ok(ElementParsePlan {
                    metadata_name: name,
                    kind: ElementParseKind::Fixed {
                        expected,
                        capture,
                        spelling: fixed_pretty_spelling(
                            &fixed.spelling,
                            lowered.grammar.text_equivalence,
                        ),
                        spacing: fixed.pretty_spacing,
                        accessor,
                    },
                })
            }
            MaterializedElement::SemanticField {
                field,
                ty,
                attachment,
                separator,
                ..
            } => {
                let resolved_field = node
                    .fields
                    .iter()
                    .find(|candidate| candidate.id == *field)
                    .ok_or_else(|| "materialized semantic field lacks source syntax".to_owned())?;
                let field_index = variant.map_or_else(
                    || {
                        node.fields
                            .iter()
                            .position(|candidate| candidate.id == *field)
                            .unwrap_or(semantic_index)
                    },
                    |variant| {
                        let resolved_variant = match &node.body {
                            ResolvedNodeBody::Enum { variants } => variants
                                .iter()
                                .find(|candidate| candidate.id == variant.resolved_variant),
                            ResolvedNodeBody::Struct { .. } => None,
                        };
                        resolved_variant
                            .and_then(|variant| {
                                variant
                                    .fields
                                    .iter()
                                    .position(|candidate| candidate == field)
                            })
                            .unwrap_or(semantic_index)
                    },
                );
                semantic_index += 1;
                let field_name = resolved_field
                    .syntax
                    .ident
                    .as_ref()
                    .map(|ident| ident.unraw().to_string())
                    .unwrap_or_else(|| format!("field_{field_index}"));
                let metadata_name = variant.map_or_else(
                    || field_name.clone(),
                    |variant| format!("{}_{}", snake_case(&variant.name), field_name),
                );
                let wrappers = field_wrappers(ty)?;
                let field_override = resolved_field.capture.map(PlannedCapturePolicy::from);
                let capture = field_override.unwrap_or(variant_capture);
                let child_capture = field_override
                    .or(variant_override)
                    .or(terminal_node_capture(ty, lowered).map(PlannedCapturePolicy::from))
                    .unwrap_or(node_capture);
                let machine = decisions
                    .container_machines()
                    .iter()
                    .find(|machine| machine.site() == *site)
                    .ok_or_else(|| format!("field `{field_name}` lacks its container machine"))?;
                validate_container_wrappers(machine.operation(), &wrappers, &field_name)?;
                let mut container = planned_container(
                    machine,
                    decisions,
                    &lowered.grammar,
                    kind_count,
                )?;
                let pratt_min_bp = pratt_operand
                    .filter(|(owner, _)| parse_site_targets(&lowered.grammar, *site, *owner))
                    .map(|(_, min_bp)| min_bp);
                if let Some((owner, min_bp)) = pratt_operand.filter(|_| pratt_min_bp.is_some()) {
                    let inactive = planned_pratt_extenders(
                        decisions,
                        owner,
                        &lowered.grammar,
                        kind_count,
                        |position| pratt_led_lbp(position) < min_bp,
                    )?;
                    match &mut container {
                        PlannedContainer::Direct { child_follow, .. } => {
                            *child_follow = follow_with_static(child_follow, &inactive);
                        }
                        PlannedContainer::Optional { .. } | PlannedContainer::Repeated { .. } => {
                            return Err(format!(
                                "Pratt operand field `{field_name}` is not a direct frozen container"
                            ));
                        }
                    }
                }
                Ok(ElementParsePlan {
                    metadata_name,
                    kind: ElementParseKind::Semantic {
                        field: FieldPlan {
                            source: resolved_field.id,
                            ident: resolved_field.syntax.ident.clone(),
                            index: field_index,
                            accessor: Some({
                                let base = resolved_field
                                    .syntax
                                    .ident
                                    .clone()
                                    .unwrap_or_else(|| format_ident!("field_{field_index}"));
                                variant.map_or(base.clone(), |variant| {
                                    let variant_name = snake_case(&variant.name);
                                    if base.unraw() == variant_name {
                                        base
                                    } else {
                                        format_ident!("{}_{}", variant_name, base)
                                    }
                                })
                            }),
                        },
                        terminal: Box::new(terminal_type(ty, lowered, input_lifetime)?),
                        wrappers,
                        container: Box::new(container),
                        capture,
                        child_capture,
                        pratt_min_bp,
                        separator: separator
                            .as_ref()
                            .map(|separator| {
                                let source = lowered.source_modeled.source_model();
                                let token_use = &source.token_uses[separator.token_use.index()];
                                let definition =
                                    &source.token_definitions[token_use.definition.index()];
                                match &definition.kind {
                                    TokenDefinitionKind::Fixed(fixed) => Ok(fixed.spelling.clone()),
                                    _ => Err(
                                        "a repetition separator is not fixed syntax".to_owned()
                                    ),
                                }
                            })
                            .transpose()?,
                        attachment: attachment
                            .as_ref()
                            .map(|attachment| {
                                plan_fixed_attachment(
                                    resolved_field,
                                    attachment,
                                    *site,
                                    lowered,
                                    decisions,
                                    kind_count,
                                    variant_capture,
                                )
                                .map(Box::new)
                            })
                            .transpose()?,
                    },
                })
            }
        })
        .collect()
}

fn parse_site_targets(
    grammar: &crate::GrammarDescription,
    site: SiteId,
    target: crate::TypeId,
) -> bool {
    let site = &grammar.sites[site.index()];
    if site.target == target {
        return true;
    }
    let TypeShape::Sequence { sites } = &grammar.types[site.target.index()].shape else {
        return false;
    };
    sites
        .iter()
        .any(|site| grammar.sites[site.index()].target == target)
}

#[allow(clippy::too_many_arguments)]
fn plan_fixed_attachment(
    field: &crate::resolution::ResolvedNodeField,
    attachment: &MaterializedFixedAttachment,
    outer_site: SiteId,
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
    variant_capture: PlannedCapturePolicy,
) -> Result<FixedAttachmentParsePlan, String> {
    let grammar = &lowered.grammar;
    let correlation = lowered
        .lowering_map
        .attachments
        .iter()
        .find(|correlation| correlation.outer == outer_site)
        .ok_or_else(|| "a fixed attachment lacks its typed M2 correlation".to_owned())?;
    if correlation.sites.len() != attachment.before.len() + 1 + attachment.after.len() {
        return Err("a fixed attachment has mismatched source and M2 unit elements".to_owned());
    }
    let capture = field
        .capture
        .map(PlannedCapturePolicy::from)
        .unwrap_or(variant_capture);
    let source = lowered.source_modeled.source_model();
    let plan_fixed = |token_use: crate::lowering::TokenUseId,
                      site: SiteId|
     -> Result<AttachedFixedParsePlan, String> {
        let token_use = &source.token_uses[token_use.index()];
        let definition = &source.token_definitions[token_use.definition.index()];
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            return Err("an attached fixed element did not reference fixed syntax".to_owned());
        };
        let PlannedFollow::Static(expected) = planned_failure_edge(
            decisions,
            site,
            FailureEdgeKind::RequiredSite,
            grammar,
            kind_count,
        )?
        else {
            return Err("an attached fixed element has a dynamic failure recipe".to_owned());
        };
        Ok(AttachedFixedParsePlan {
            expected,
            capture,
            spelling: fixed_pretty_spelling(&fixed.spelling, grammar.text_equivalence),
            spacing: fixed.pretty_spacing,
        })
    };
    let before = attachment
        .before
        .iter()
        .zip(&correlation.sites)
        .map(|(token_use, site)| plan_fixed(*token_use, *site))
        .collect::<Result<Vec<_>, _>>()?;
    let semantic_site = correlation.sites[correlation.semantic_index];
    let machine = decisions
        .container_machines()
        .iter()
        .find(|machine| machine.site() == semantic_site)
        .ok_or_else(|| {
            "an attachment semantic site lacks its direct container machine".to_owned()
        })?;
    if machine.operation() != ContainerOperation::Direct {
        return Err("an attachment semantic site is not a direct frozen container".to_owned());
    }
    let semantic = Box::new(planned_container(machine, decisions, grammar, kind_count)?);
    let after_start = correlation.semantic_index + 1;
    let after = attachment
        .after
        .iter()
        .zip(&correlation.sites[after_start..])
        .map(|(token_use, site)| plan_fixed(*token_use, *site))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FixedAttachmentParsePlan {
        before,
        semantic,
        after,
    })
}

fn planned_pratt_extenders(
    decisions: &crate::DecisionProducts,
    owner: crate::TypeId,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
    include: impl Fn(PrattPosition) -> bool,
) -> Result<ExpectedSet, String> {
    let edge = decisions
        .failure_edges()
        .iter()
        .find(|edge| {
            edge.owner() == owner && edge.kind() == FailureEdgeKind::PrattCallerFollowGuard
        })
        .ok_or_else(|| "a Pratt operand lacks its frozen extender family".to_owned())?;
    let ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches } = edge.expected() else {
        return Err("a Pratt operand has an invalid frozen extender family".to_owned());
    };
    let first = branches
        .iter()
        .filter(|branch| branch.position().is_some_and(&include))
        .flat_map(|branch| branch.sequences())
        .filter_map(|sequence| sequence.positions().first().cloned())
        .collect::<Vec<_>>();
    ExpectedSet::from_positions(&first, grammar, kind_count)
}

fn terminal_node_capture(
    ty: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
) -> Option<CaptureMode> {
    match ty {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => terminal_node_capture(inner, lowered),
        ClosedType::Node(node_use) => {
            let node = lowered.applications[node_use.application.index()].node;
            lowered
                .source_modeled
                .resolved()
                .nodes
                .iter()
                .find(|candidate| candidate.id == node)
                .and_then(|node| node.capture)
        }
        ClosedType::ContentToken(_) => None,
    }
}

#[cfg(feature = "diagrams")]
fn diagram_terminal_name(
    ty: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<String, String> {
    match ty {
        ClosedType::Node(node) => lowered
            .applications
            .get(node.application.index())
            .map(|application| application.display_name.clone())
            .ok_or_else(|| "diagram terminal references an unknown Node application".to_owned()),
        ClosedType::ContentToken(token) => lowered
            .source_modeled
            .resolved()
            .content_tokens
            .iter()
            .find(|content| content.id == *token)
            .map(|content| content.ident.clone())
            .ok_or_else(|| "diagram terminal references an unknown content token".to_owned()),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => diagram_terminal_name(inner, lowered),
    }
}

#[cfg(feature = "diagrams")]
fn diagram_separator_spelling(
    separator: Option<&crate::source_model::Separator>,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<Option<String>, String> {
    let Some(separator) = separator else {
        return Ok(None);
    };
    let source_model = lowered.source_modeled.source_model();
    let token_use = source_model
        .token_uses
        .get(separator.token_use.index())
        .ok_or_else(|| "diagram separator references an unknown token use".to_owned())?;
    let definition = source_model
        .token_definitions
        .get(token_use.definition.index())
        .ok_or_else(|| "diagram separator references an unknown token definition".to_owned())?;
    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
        return Err("diagram separator does not reference fixed-token syntax".to_owned());
    };
    Ok(Some(fixed.spelling.clone()))
}

fn fixed_element_name(
    node: &ResolvedNode,
    token_use: &crate::source_model::TokenUse,
    definition: &crate::source_model::TokenDefinition,
    fixed: &crate::source_model::FixedTokenDefinition,
) -> String {
    let field = fixed_element_field(node, token_use);
    let base = if definition.name.starts_with('"') {
        snake_case(&fixed.spelling)
    } else {
        snake_case(&definition.name)
    };
    field
        .and_then(|field| field.syntax.ident.as_ref())
        .map_or(base.clone(), |field| {
            format!("{}_{}", snake_case(&field.unraw().to_string()), base)
        })
}

fn fixed_element_field<'node>(
    node: &'node ResolvedNode,
    token_use: &crate::source_model::TokenUse,
) -> Option<&'node crate::resolution::ResolvedNodeField> {
    node.fields.iter().find(|field| {
        field.metas.iter().any(|meta| {
            let range = span_range(meta.span());
            range.start <= token_use.site.range.start && range.end >= token_use.site.range.end
        })
    })
}

fn field_wrappers(ty: &ClosedType) -> Result<Vec<FieldWrapper>, String> {
    fn walk(ty: &ClosedType, output: &mut Vec<FieldWrapper>) {
        match ty {
            ClosedType::Node(_) | ClosedType::ContentToken(_) => {}
            ClosedType::Boxed(inner) => {
                output.push(FieldWrapper::Boxed);
                walk(inner, output)
            }
            ClosedType::Optional(inner) => {
                output.push(FieldWrapper::Optional);
                walk(inner, output)
            }
            ClosedType::ZeroOrMore(inner) => {
                output.push(FieldWrapper::ZeroOrMore);
                walk(inner, output)
            }
            ClosedType::OneOrMore(inner) => {
                output.push(FieldWrapper::OneOrMore);
                walk(inner, output)
            }
        }
    }
    let mut wrappers = Vec::new();
    walk(ty, &mut wrappers);
    Ok(wrappers)
}

fn validate_container_wrappers(
    operation: ContainerOperation,
    wrappers: &[FieldWrapper],
    field_name: &str,
) -> Result<(), String> {
    let repetition = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore));
    let optional_before_repeat = repetition.is_some_and(|index| {
        wrappers[..index]
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    });
    let inferred = match repetition.map(|index| wrappers[index]) {
        Some(FieldWrapper::ZeroOrMore) => ContainerOperation::ZeroOrMore,
        Some(FieldWrapper::OneOrMore) if optional_before_repeat => {
            ContainerOperation::OptionalOneOrMore
        }
        Some(FieldWrapper::OneOrMore) => ContainerOperation::OneOrMore,
        Some(FieldWrapper::Boxed | FieldWrapper::Optional) => unreachable!(),
        None if wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional)) =>
        {
            ContainerOperation::Optional
        }
        None => ContainerOperation::Direct,
    };
    if inferred == operation {
        Ok(())
    } else {
        Err(format!(
            "field `{field_name}` container wrappers disagree with its frozen machine: {inferred:?} vs {operation:?}"
        ))
    }
}

fn planned_container(
    machine: &crate::ContainerMachine,
    decisions: &crate::DecisionProducts,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedContainer, String> {
    let stop = planned_follow(machine.stop(), grammar, kind_count)?;
    let element_selector =
        ExpectedSet::from_positions(machine.element_first(), grammar, kind_count)?;
    Ok(match machine.operation() {
        ContainerOperation::Direct => PlannedContainer::Direct {
            child_follow: stop,
            required_failure: planned_failure_edge(
                decisions,
                machine.site(),
                FailureEdgeKind::RequiredSite,
                grammar,
                kind_count,
            )?,
        },
        ContainerOperation::Optional => PlannedContainer::Optional {
            selector: element_selector,
            child_follow: stop,
        },
        operation @ (ContainerOperation::ZeroOrMore
        | ContainerOperation::OneOrMore
        | ContainerOperation::OptionalOneOrMore) => {
            let separator_selector = if machine.separator().is_some() {
                Some(ExpectedSet::from_positions(
                    machine.separator_first(),
                    grammar,
                    kind_count,
                )?)
            } else {
                None
            };
            let continuation = separator_selector
                .as_ref()
                .unwrap_or(&element_selector)
                .clone();
            let child_follow = follow_with_static(&stop, &continuation);
            PlannedContainer::Repeated {
                operation,
                element_selector,
                separator_selector,
                optional_trailing: machine.optional_trailing(),
                stop,
                child_follow,
                minimum_failure: matches!(
                    operation,
                    ContainerOperation::OneOrMore | ContainerOperation::OptionalOneOrMore
                )
                .then(|| {
                    planned_failure_edge(
                        decisions,
                        machine.site(),
                        FailureEdgeKind::ContainerMinimum,
                        grammar,
                        kind_count,
                    )
                })
                .transpose()?
                .map(Box::new),
                continuation_failure: Box::new(planned_failure_edge(
                    decisions,
                    machine.site(),
                    FailureEdgeKind::ContainerContinuation,
                    grammar,
                    kind_count,
                )?),
                after_separator_failure: machine
                    .separator()
                    .map(|_| {
                        planned_failure_edge(
                            decisions,
                            machine.site(),
                            FailureEdgeKind::ContainerAfterSeparator,
                            grammar,
                            kind_count,
                        )
                    })
                    .transpose()?
                    .map(Box::new),
            }
        }
    })
}

fn planned_failure_edge(
    decisions: &crate::DecisionProducts,
    site: SiteId,
    kind: FailureEdgeKind,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    let edge = decisions
        .failure_edges()
        .iter()
        .find(|edge| edge.site() == Some(site) && edge.kind() == kind)
        .ok_or_else(|| {
            format!(
                "site {} lacks its frozen {kind:?} failure edge",
                site.index()
            )
        })?;
    planned_expected_recipe(edge.expected(), grammar, kind_count)
}

fn planned_expected_recipe(
    expected: &ExpectedRecipe,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    match expected {
        ExpectedRecipe::Static(first) => Ok(PlannedFollow::Static(ExpectedSet::from_positions(
            first, grammar, kind_count,
        )?)),
        ExpectedRecipe::Follow(follow) => planned_follow(follow, grammar, kind_count),
        ExpectedRecipe::ContinueOrStop { continuation, stop } => {
            let stop = planned_follow(stop, grammar, kind_count)?;
            let continuation = ExpectedSet::from_positions(continuation, grammar, kind_count)?;
            Ok(follow_with_static(&stop, &continuation))
        }
        ExpectedRecipe::PrattExtendersAtOrAboveMinBp { .. } => {
            Err("a non-Pratt site referenced a Pratt failure recipe".to_owned())
        }
    }
}

fn terminal_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    match closed {
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => terminal_type(inner, lowered, input_lifetime),
        ClosedType::Node(node_use) => materialized_application_type(
            &lowered.applications[node_use.application.index()],
            &node_use.lifetimes,
            lowered,
            input_lifetime,
        ),
        ClosedType::ContentToken(token) => {
            let mut path: syn::Path = syn::parse_quote!(crate);
            for segment in &lowered
                .source_modeled
                .resolved()
                .build_inputs
                .module_anchor
                .segments
            {
                path.segments
                    .push(syn::PathSegment::from(emitted_ident(segment)?));
            }
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__recursa_content_tokens"),
                arguments: PathArguments::None,
            });
            path.segments.push(syn::PathSegment {
                ident: format_ident!("__RecursaContentToken{}", token.index()),
                arguments: PathArguments::AngleBracketed(syn::parse_quote!(<#input_lifetime>)),
            });
            Ok(Type::Path(syn::TypePath { qself: None, path }))
        }
    }
}

fn materialized_application_type(
    application: &MaterializedNodeApplication,
    lifetimes: &[syn::Lifetime],
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let resolved = lowered.source_modeled.resolved();
    let node = &resolved.nodes[application.node.index()];
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in &node.key.segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "materialized application path is empty".to_owned())?;
    let mut lifetime_arguments = lifetimes.iter();
    let mut type_arguments = application.type_arguments.iter();
    let mut arguments = syn::punctuated::Punctuated::new();
    for parameter in &node.generics.params {
        match parameter {
            GenericParam::Lifetime(_) => {
                let lifetime = lifetime_arguments.next().ok_or_else(|| {
                    format!(
                        "materialized application `{}` lacks an authored lifetime argument",
                        application.display_name
                    )
                })?;
                arguments.push(GenericArgument::Lifetime(lifetime.clone()));
            }
            GenericParam::Type(_) => {
                let argument = type_arguments.next().ok_or_else(|| {
                    format!(
                        "materialized application `{}` lacks a closed type argument",
                        application.display_name
                    )
                })?;
                arguments.push(GenericArgument::Type(closed_type(
                    argument,
                    lowered,
                    input_lifetime,
                )?));
            }
            GenericParam::Const(_) => {
                return Err("const-generic materialized applications cannot be emitted".to_owned());
            }
        }
    }
    if lifetime_arguments.next().is_some() || type_arguments.next().is_some() {
        return Err(format!(
            "materialized application `{}` has excess generic arguments",
            application.display_name
        ));
    }
    if !arguments.is_empty() {
        segment.arguments = PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: arguments,
            gt_token: Default::default(),
        });
    }
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

fn closed_type(
    closed: &ClosedType,
    lowered: &crate::materialization::LoweredGrammar,
    input_lifetime: &syn::Lifetime,
) -> Result<Type, String> {
    let facade = emitted_ident(&lowered.source_modeled.resolved().build_inputs.facade.ident)?;
    Ok(match closed {
        ClosedType::Node(_) | ClosedType::ContentToken(_) => {
            terminal_type(closed, lowered, input_lifetime)?
        }
        ClosedType::Boxed(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Box<#inner>)
        }
        ClosedType::Optional(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Option<#inner>)
        }
        ClosedType::ZeroOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(Vec<#inner>)
        }
        ClosedType::OneOrMore(inner) => {
            let inner = closed_type(inner, lowered, input_lifetime)?;
            syn::parse_quote!(#facade::Vec1<#inner>)
        }
    })
}

fn planned_follow(
    recipe: &FollowRecipe,
    grammar: &crate::GrammarDescription,
    kind_count: usize,
) -> Result<PlannedFollow, String> {
    Ok(match recipe {
        FollowRecipe::Caller => PlannedFollow::Caller,
        FollowRecipe::Static { first } => {
            PlannedFollow::Static(ExpectedSet::from_positions(first, grammar, kind_count)?)
        }
        FollowRecipe::StaticAndCaller { first } => {
            PlannedFollow::StaticAndCaller(ExpectedSet::from_positions(first, grammar, kind_count)?)
        }
    })
}

fn follow_with_static(follow: &PlannedFollow, additional: &ExpectedSet) -> PlannedFollow {
    match follow {
        PlannedFollow::Caller => PlannedFollow::StaticAndCaller(additional.clone()),
        PlannedFollow::Static(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::Static(expected)
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let mut expected = expected.clone();
            expected.union_assign(additional);
            PlannedFollow::StaticAndCaller(expected)
        }
    }
}

fn validate_accessor_names(node: &ResolvedNode, body: &NodeBodyPlan) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut check = |element: &ElementParsePlan| -> Result<(), String> {
        let name = match &element.kind {
            ElementParseKind::Fixed { accessor, .. } => accessor.unraw().to_string(),
            ElementParseKind::Semantic { field, .. } => {
                let Some(accessor) = &field.accessor else {
                    return Ok(());
                };
                accessor.unraw().to_string()
            }
        };
        if !names.insert(name.clone()) {
            return Err(format!(
                "generated accessor `{name}` collides within Node `{}`",
                node.ident
            ));
        }
        Ok(())
    };
    match body {
        NodeBodyPlan::Struct { elements, .. } => {
            for element in elements {
                check(element)?;
            }
        }
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                for element in &variant.elements {
                    check(element)?;
                }
            }
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &pratt.nud {
                for element in &branch.variant.elements {
                    check(element)?;
                }
            }
            for branch in &pratt.led {
                check(&branch.lhs)?;
                for element in &branch.variant.elements {
                    check(element)?;
                }
            }
        }
    }
    Ok(())
}

struct RenderedGeneration {
    root: String,
    nodes: Vec<(String, String)>,
    node_anchors: Vec<String>,
    selector: String,
}

fn render(plan: &EmissionPlan) -> Result<RenderedGeneration, String> {
    plan.validate()?;
    let root = render_root(plan).to_string();
    let mut instrumentation = RenderInstrumentation::default();
    let mut nodes = Vec::new();
    for (index, node) in plan.nodes.iter().enumerate() {
        let name = format!("node-{index}.rs");
        let rendered = render_node(plan, node, index, &mut instrumentation).to_string();
        syn::parse_file(&rendered)
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        nodes.push((name, rendered));
    }
    if instrumentation.frozen_dispatch_nodes_rendered
        != plan.instrumentation.frozen_dispatch_nodes_consumed
    {
        return Err("the renderer did not consume exactly the frozen dispatch trie".to_owned());
    }
    let selected_root = format!("/recursa-generations/{}/root.rs", plan.identity);
    let selector_body = quote! {
        include!(concat!(env!("OUT_DIR"), #selected_root));
    }
    .to_string();
    let selector = format!("// recursa-generation:{}\n{selector_body}", plan.identity);
    syn::parse_file(&root).map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    syn::parse_file(&selector)
        .map_err(|error| format!("Generation selector rendering is invalid Rust: {error}"))?;
    Ok(RenderedGeneration {
        root,
        nodes,
        node_anchors: plan
            .nodes
            .iter()
            .map(|node| node.loader_anchor.clone())
            .collect(),
        selector,
    })
}

#[cfg(feature = "diagrams")]
fn render_aggregate_diagram_document(plan: &DiagramPlan) -> String {
    let mut document = format!(
        "# Generated grammar diagram\n\nRECURSA_GRAMMAR_DIAGRAM\n\nGrammar `{}` is rendered from the frozen semantic Generation plan.\n\n",
        plan.grammar_name
    );
    for node in &plan.nodes {
        document.push_str(&diagram_svg(&node.production, &node.semantic_key));
        document.push_str("\n\n");
    }
    document.push_str("## Public Nodes\n\n");
    for node in plan.nodes.iter().filter(|node| node.public_path.is_some()) {
        let path = node.public_path.as_deref().expect("filtered public path");
        document.push_str(&format!("- [`{}`]({path})\n", node.semantic_key));
    }
    document.push_str(
        "\n## Fixed tokens\n\nFixed tokens are syntax-only: they shape accepted input but do not create AST fields or retain matched text. Their canonical spellings are:\n\n",
    );
    for (name, spelling) in &plan.fixed_tokens {
        document.push_str(&format!("- `{name}`: `{spelling}`\n"));
    }
    document
}

#[cfg(feature = "diagrams")]
fn render_node_diagram_document(node: &NodeDiagramPlan) -> String {
    format!(
        "## Generated grammar diagram\n\nRECURSA_NODE_DIAGRAM: {}\n\n{}\n\nFixed tokens shown as quoted terminals are syntax-only. They shape parsing but do not add fields to the authored Node or retain matched text.\n",
        node.ident,
        diagram_svg(&node.production, &node.semantic_key),
    )
}

#[cfg(feature = "diagrams")]
fn diagram_svg(production: &str, label: &str) -> String {
    let width = 40_usize
        .saturating_add(production.chars().count().saturating_mul(9))
        .clamp(240, 1400);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"52\" role=\"img\" aria-label=\"{} grammar diagram\"><rect x=\"1\" y=\"1\" width=\"{}\" height=\"50\" rx=\"5\" fill=\"none\" stroke=\"currentColor\"/><text x=\"16\" y=\"32\" font-family=\"monospace\" fill=\"currentColor\">{}</text></svg>",
        escape_html(label),
        width.saturating_sub(2),
        escape_html(production),
    )
}

#[cfg(feature = "diagrams")]
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_root(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let release_text = &plan.private_abi.release;
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let private_protocol = plan.private_abi.protocol;
    let protocol = format_ident!("protocol_{private_protocol}");
    let grammar_name = &plan.grammar_name;
    #[cfg(feature = "diagrams")]
    let aggregate_diagram = plan.diagrams.as_ref().map(|diagrams| {
        let documentation = syn::LitStr::new(
            &render_aggregate_diagram_document(diagrams),
            proc_macro2::Span::call_site(),
        );
        quote! {
            #[doc = #documentation]
            pub mod grammar_docs {}
        }
    });
    #[cfg(not(feature = "diagrams"))]
    let aggregate_diagram = proc_macro2::TokenStream::new();
    let kind_count = plan.kind_count;
    let lexical_error_kind = plan.lexical_error_kind;
    let kind_names = &plan.kind_names;
    let atom_names = &plan.atom_names;
    let kind_variants = &plan.kind_variants;
    let kind_match_arms = kind_variants.iter().enumerate().map(|(index, variant)| {
        let index = index as u16;
        quote!(#index => TokenKind::#variant,)
    });
    let raw_kind_arms = kind_variants.iter().enumerate().map(|(index, variant)| {
        let index = index as u16;
        quote!(TokenKind::#variant => #index,)
    });
    let kind_static_arms = kind_variants
        .iter()
        .zip(&plan.kind_static_texts)
        .filter_map(|(variant, text)| {
            let text = text.as_ref()?;
            Some(quote!(TokenKind::#variant => Some(#text),))
        });
    let kind_display_arms =
        kind_variants
            .iter()
            .zip(&plan.kind_static_texts)
            .map(|(variant, text)| {
                if let Some(text) = text {
                    quote!(TokenKind::#variant => formatter.write_str(#text),)
                } else {
                    quote!(TokenKind::#variant => formatter.write_str(stringify!(#variant)),)
                }
            });
    let word_candidate_kinds = &plan.word_candidate_kinds;
    let word_classifiers = plan.word_classifiers.iter().map(|classifier| {
        let canonical = &classifier.canonical_spelling;
        let kind = classifier.kind;
        let matches = if classifier.ascii_insensitive {
            quote!(text.eq_ignore_ascii_case(#canonical))
        } else {
            quote!(text == #canonical)
        };
        quote! {
            if source
                .get(record.span.range())
                .is_some_and(|text| #matches)
            {
                record.kind = #kind;
                return;
            }
        }
    });
    let post_lex = plan.post_lex.as_ref().map(|hook| {
        quote! {
            let mut editor = LexEditor { source, records };
            #hook(&mut editor);
            records = editor.records;
        }
    });
    let patterns = plan.lexical_rules.iter().map(|rule| &rule.pattern);
    let kinds = plan.lexical_rules.iter().map(|rule| rule.kind);
    let skips = plan.lexical_rules.iter().map(|rule| rule.skip);
    let fixed = plan.lexical_rules.iter().map(|rule| rule.fixed);
    let priorities = plan.lexical_rules.iter().map(|rule| rule.priority);
    let callbacks = plan.lexical_rules.iter().enumerate().map(|(index, rule)| {
        if rule.callback.is_some() {
            let shim = format_ident!("__recursa_lex_callback_{index}");
            quote!(Some(#shim))
        } else {
            quote!(None)
        }
    });
    let callback_shims = plan
        .lexical_rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            let callback = rule.callback.as_ref()?;
            let shim = format_ident!("__recursa_lex_callback_{index}");
            Some(quote! {
                fn #shim(
                    context: &mut #facade::__private::LexContext<'_>,
                ) -> #facade::__private::LexAction {
                    let initial_end = context.end();
                    let mut typed = LexContext {
                        source: context.source(),
                        start: context.start(),
                        end: initial_end,
                    };
                    let action = #callback(&mut typed);
                    context
                        .bump(typed.end - initial_end)
                        .expect("generated typed callback context validated its bump");
                    match action {
                        super::LexAction::Emit => #facade::__private::LexAction::Emit,
                        super::LexAction::EmitAs(kind) => {
                            #facade::__private::LexAction::EmitAs(__recursa_raw_kind(kind))
                        }
                        super::LexAction::Skip => #facade::__private::LexAction::Skip,
                        super::LexAction::Error => #facade::__private::LexAction::Error,
                    }
                }
            })
        });
    let empty_words = vec![0_u64; kind_count.div_ceil(64)];
    let token_starts = plan.tokens.iter().enumerate().map(|(index, token)| {
        let name = format_ident!("__RECURSA_TOKEN_START_{index}");
        render_expected_static(&name, &token.expected, plan, facade)
    });
    let token_items = plan
        .tokens
        .iter()
        .enumerate()
        .map(|(index, token)| render_token(plan, token, index));
    let visit_events = render_visit_events(plan);
    let loader_arms = plan.nodes.iter().enumerate().map(|(index, node)| {
        let ident = &node.ident;
        let anchor = syn::LitStr::new(&node.loader_anchor, proc_macro2::Span::call_site());
        let path = format!("/recursa-generations/{}/node-{index}.rs", plan.identity);
        quote! {
            (#ident, #anchor) => {
                include!(concat!(env!("OUT_DIR"), #path));
            };
        }
    });
    let capability_arms = plan.capabilities.iter().map(|capability| {
        let diagrams = u8::from(capability.diagrams);
        let explorer = u8::from(capability.explorer);
        let arbitrary = u8::from(capability.arbitrary);
        let serde = u8::from(capability.serde);
        let capability = format_ident!("cap_core_d{diagrams}_e{explorer}_a{arbitrary}_s{serde}");
        quote! {
            (#release, #protocol, #capability) => {
                const _: #facade::__private::FacadeCompatibility<
                    { #facade::__private::release_stamp(#release_text) },
                    #private_protocol,
                > = #facade::__private::FACADE_COMPATIBILITY;
            };
        }
    });

    quote! {
        #[doc(hidden)]
        pub(crate) struct __RecursaGenerationAnchor;
        const _: fn() = || {
            let _: #module_anchor::__RecursaGenerationAnchor = __RecursaGenerationAnchor;
        };

        #aggregate_diagram

        static __RECURSA_GRAMMAR: #facade::__private::GrammarInfo =
            #facade::__private::grammar_info(
                #grammar_name,
                &[#(#kind_names),*],
                &[#(#atom_names),*],
                #lexical_error_kind,
            );
        static __RECURSA_EMPTY_FOLLOW: #facade::__private::FollowSet =
            #facade::__private::follow_set(
                #kind_count as u32,
                &[#(#empty_words),*],
                &[],
            );
        #(#token_starts)*

        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[allow(non_camel_case_types)]
        pub enum TokenKind {
            #(#kind_variants,)*
            LexicalError,
        }

        impl TokenKind {
            /// Static spelling for a data-free fixed token kind.
            pub const fn static_text(self) -> Option<&'static str> {
                match self {
                    #(#kind_static_arms)*
                    _ => None,
                }
            }
        }

        impl ::core::fmt::Display for TokenKind {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #(#kind_display_arms)*
                    TokenKind::LexicalError => formatter.write_str("LexicalError"),
                }
            }
        }

        /// The complete set of decisions a lexer callback can make.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexAction {
            /// Emit the rule's declared token kind.
            Emit,
            /// Emit a different named kind from this grammar.
            EmitAs(TokenKind),
            /// Consume the matched source without emitting a record.
            Skip,
            /// Emit this grammar's lexical-error record.
            Error,
        }

        /// A cursor bound to this generated grammar's token identities.
        pub struct Input<'input, A = ()> {
            pub(crate) inner: #facade::__private::Input<'input, A>,
        }

        impl<'input, A> Input<'input, A> {
            pub fn source(&self) -> &'input str {
                self.inner.source()
            }

            pub fn builder(&mut self) -> &mut A {
                self.inner.builder()
            }

            pub fn cursor(&self) -> usize {
                self.inner.cursor()
            }

            pub fn is_eof(&self) -> bool {
                self.inner.is_eof()
            }
        }

        mod __recursa_lexical {
            use super::{Input, TokenKind};

            static __RECURSA_LEX_RULES: &[#facade::__private::LexRule] = &[
                #(
                    #facade::__private::LexRule {
                        pattern: #patterns,
                        kind: #kinds,
                        skip: #skips,
                        callback: #callbacks,
                        fixed: #fixed,
                        priority: #priorities,
                    }
                ),*
            ];

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexBumpError {
            _private: (),
        }

        impl ::core::fmt::Display for LexBumpError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(
                    "lexer callback bump is outside the remaining UTF-8 source",
                )
            }
        }

        impl ::std::error::Error for LexBumpError {}

        /// Backend-independent access to one callback's current match.
        pub struct LexContext<'source> {
            source: &'source str,
            start: usize,
            end: usize,
        }

        impl<'source> LexContext<'source> {
            /// The complete source slice currently claimed by this callback.
            pub fn slice(&self) -> &str {
                &self.source[self.start..self.end]
            }

            /// Source following the callback's current slice.
            pub fn remainder(&self) -> &str {
                &self.source[self.end..]
            }

            /// Extends the current slice by a validated byte count.
            ///
            /// Failure leaves the slice unchanged. Bumps may not exceed the
            /// source or end between UTF-8 code units.
            pub fn bump(&mut self, bytes: usize) -> Result<(), LexBumpError> {
                let Some(end) = self.end.checked_add(bytes) else {
                    return Err(LexBumpError { _private: () });
                };
                if end > self.source.len() || !self.source.is_char_boundary(end) {
                    return Err(LexBumpError { _private: () });
                }
                self.end = end;
                Ok(())
            }
        }

        #(#callback_shims)*

        fn __recursa_raw_kind(kind: TokenKind) -> u16 {
            match kind {
                #(#raw_kind_arms)*
                TokenKind::LexicalError => #lexical_error_kind,
            }
        }

        fn __recursa_typed_kind(kind: u16) -> TokenKind {
            match kind {
                #(#kind_match_arms)*
                _ => TokenKind::LexicalError,
            }
        }

        fn __recursa_classify(
            source: &str,
            record: &mut #facade::__private::TokenRecord,
        ) {
            if ![#(#word_candidate_kinds),*].contains(&record.kind) {
                return;
            }
            #(#word_classifiers)*
        }

        fn __recursa_finalize<'input>(
            source: &'input str,
            mut records: Vec<#facade::__private::TokenRecord>,
        ) -> #facade::__private::LexResult<'input> {
            for record in &mut records {
                __recursa_classify(source, record);
            }
            #post_lex
            #facade::__private::LexResult::new(source, records)
        }

        /// Read-only metadata for this generated grammar.
        pub struct GrammarInfo {
            _private: (),
        }

        impl GrammarInfo {
            pub fn name(&self) -> &'static str {
                super::__RECURSA_GRAMMAR.name()
            }

        }

        static __RECURSA_PUBLIC_GRAMMAR_INFO: GrammarInfo = GrammarInfo { _private: () };

        pub fn grammar_info() -> &'static GrammarInfo {
            &__RECURSA_PUBLIC_GRAMMAR_INFO
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Token<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> Token<'input> {
            pub fn kind(self) -> TokenKind {
                __recursa_typed_kind(self.record.kind)
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("generated lexer produced a valid UTF-8 span")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }
        }

        impl ::core::fmt::Display for Token<'_> {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.source.get(self.record.span.range()).ok_or(::core::fmt::Error)?)
            }
        }

        pub struct Tokens<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
            lexical_error_kind: u16,
        }

        impl<'input> Iterator for Tokens<'_, 'input> {
            type Item = Token<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records
                    .find(|record| record.kind != self.lexical_error_kind)
                    .copied()
                    .map(|record| Token {
                        source: self.source,
                        record,
                    })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self
                    .records
                    .as_slice()
                    .iter()
                    .filter(|record| record.kind != self.lexical_error_kind)
                    .count();
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for Tokens<'_, '_> {}

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexError<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> LexError<'input> {
            pub const fn kind(self) -> TokenKind {
                TokenKind::LexicalError
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("generated lexer produced a valid UTF-8 error span")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }
        }

        impl ::core::fmt::Display for LexError<'_> {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.source.get(self.record.span.range()).ok_or(::core::fmt::Error)?)
            }
        }

        pub struct LexErrors<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
            lexical_error_kind: u16,
        }

        impl<'input> Iterator for LexErrors<'_, 'input> {
            type Item = LexError<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records
                    .find(|record| record.kind == self.lexical_error_kind)
                    .copied()
                    .map(|record| LexError {
                        source: self.source,
                        record,
                    })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self
                    .records
                    .as_slice()
                    .iter()
                    .filter(|record| record.kind == self.lexical_error_kind)
                    .count();
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for LexErrors<'_, '_> {}

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexBuildError {
            InvalidSpan { span: #facade::Span },
            OutOfOrder {
                previous: #facade::Span,
                span: #facade::Span,
            },
        }

        impl ::core::fmt::Display for LexBuildError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InvalidSpan { span } => write!(
                        formatter,
                        "token span {}..{} is not a UTF-8 range within the lexer source",
                        span.start(),
                        span.end(),
                    ),
                    Self::OutOfOrder { previous, span } => write!(
                        formatter,
                        "token span {}..{} overlaps or precedes {}..{}",
                        span.start(),
                        span.end(),
                        previous.start(),
                        previous.end(),
                    ),
                }
            }
        }

        impl ::std::error::Error for LexBuildError {}

        /// Validated input for an alternative lexer.
        pub struct LexBuilder<'input> {
            source: &'input str,
            records: Vec<#facade::__private::TokenRecord>,
        }

        impl<'input> LexBuilder<'input> {
            pub fn new(source: &'input str) -> Self {
                Self {
                    source,
                    records: Vec::new(),
                }
            }

            pub fn append(
                &mut self,
                kind: TokenKind,
                span: #facade::Span,
            ) -> Result<&mut Self, LexBuildError> {
                if self.source.get(span.range()).is_none() {
                    return Err(LexBuildError::InvalidSpan { span });
                }
                if let Some(previous) = self.records.last().map(|record| record.span) {
                    if span.start() < previous.end() {
                        return Err(LexBuildError::OutOfOrder { previous, span });
                    }
                }
                self.records.push(#facade::__private::TokenRecord {
                    kind: __recursa_raw_kind(kind),
                    span,
                });
                Ok(self)
            }

            pub fn finish(self) -> LexResult<'input> {
                LexResult {
                    inner: __recursa_finalize(self.source, self.records),
                }
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexRecord<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> LexRecord<'input> {
            pub fn kind(self) -> TokenKind {
                __recursa_typed_kind(self.record.kind)
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("the lexical editor preserves valid UTF-8 spans")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexEditError {
            MissingRecord { index: usize },
            EmptyReplacement,
            InvalidSpan { span: #facade::Span },
            OutOfOrder {
                previous: #facade::Span,
                span: #facade::Span,
            },
            InvalidRange { start: usize, end: usize },
        }

        impl ::core::fmt::Display for LexEditError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::MissingRecord { index } => {
                        write!(formatter, "lexical record index {index} does not exist")
                    }
                    Self::EmptyReplacement => {
                        formatter.write_str("lexical replacement must contain a record")
                    }
                    Self::InvalidSpan { span } => write!(
                        formatter,
                        "replacement span {}..{} is outside its source record",
                        span.start(),
                        span.end(),
                    ),
                    Self::OutOfOrder { previous, span } => write!(
                        formatter,
                        "replacement span {}..{} overlaps or precedes {}..{}",
                        span.start(),
                        span.end(),
                        previous.start(),
                        previous.end(),
                    ),
                    Self::InvalidRange { start, end } => {
                        write!(formatter, "lexical record range {start}..{end} is invalid")
                    }
                }
            }
        }

        impl ::std::error::Error for LexEditError {}

        /// Validated, grammar-typed mutation available only during post-lex.
        pub struct LexEditor<'input> {
            source: &'input str,
            records: Vec<#facade::__private::TokenRecord>,
        }

        impl<'input> LexEditor<'input> {
            pub fn source(&self) -> &'input str {
                self.source
            }

            pub fn len(&self) -> usize {
                self.records.len()
            }

            pub fn is_empty(&self) -> bool {
                self.records.is_empty()
            }

            pub fn record(&self, index: usize) -> Option<LexRecord<'input>> {
                self.records.get(index).copied().map(|record| LexRecord {
                    source: self.source,
                    record,
                })
            }

            pub fn reclassify(
                &mut self,
                index: usize,
                kind: TokenKind,
            ) -> Result<(), LexEditError> {
                let Some(record) = self.records.get_mut(index) else {
                    return Err(LexEditError::MissingRecord { index });
                };
                record.kind = __recursa_raw_kind(kind);
                __recursa_classify(self.source, record);
                Ok(())
            }

            pub fn replace<I>(
                &mut self,
                index: usize,
                replacement: I,
            ) -> Result<(), LexEditError>
            where
                I: IntoIterator<Item = (TokenKind, #facade::Span)>,
            {
                let Some(original) = self.records.get(index).copied() else {
                    return Err(LexEditError::MissingRecord { index });
                };
                let mut replacement = replacement
                    .into_iter()
                    .map(|(kind, span)| #facade::__private::TokenRecord {
                        kind: __recursa_raw_kind(kind),
                        span,
                    })
                    .collect::<Vec<_>>();
                if replacement.is_empty() {
                    return Err(LexEditError::EmptyReplacement);
                }
                let mut previous: Option<#facade::Span> = None;
                for record in &replacement {
                    if record.span.start() < original.span.start()
                        || record.span.end() > original.span.end()
                        || self.source.get(record.span.range()).is_none()
                    {
                        return Err(LexEditError::InvalidSpan { span: record.span });
                    }
                    if let Some(previous_span) = previous {
                        if record.span.start() < previous_span.end() {
                            return Err(LexEditError::OutOfOrder {
                                previous: previous_span,
                                span: record.span,
                            });
                        }
                    }
                    previous = Some(record.span);
                }
                for record in &mut replacement {
                    __recursa_classify(self.source, record);
                }
                self.records.splice(index..index + 1, replacement);
                Ok(())
            }

            pub fn remove(
                &mut self,
                range: ::core::ops::Range<usize>,
            ) -> Result<(), LexEditError> {
                if range.start > range.end || range.end > self.records.len() {
                    return Err(LexEditError::InvalidRange {
                        start: range.start,
                        end: range.end,
                    });
                }
                self.records.drain(range);
                Ok(())
            }

            pub fn classify_word(&mut self, index: usize) -> Result<(), LexEditError> {
                let Some(record) = self.records.get_mut(index) else {
                    return Err(LexEditError::MissingRecord { index });
                };
                __recursa_classify(self.source, record);
                Ok(())
            }
        }

        pub struct LexResult<'input> {
            inner: #facade::__private::LexResult<'input>,
        }

        impl<'input> LexResult<'input> {
            pub fn tokens(&self) -> Tokens<'_, 'input> {
                Tokens {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                    lexical_error_kind: #lexical_error_kind,
                }
            }

            pub fn errors(&self) -> LexErrors<'_, 'input> {
                LexErrors {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                    lexical_error_kind: #lexical_error_kind,
                }
            }

            pub fn source(&self) -> &'input str {
                #facade::__private::lex_source(&self.inner)
            }

            pub fn input(&self) -> Input<'input> {
                Input {
                    inner: self.inner.input(&super::__RECURSA_GRAMMAR),
                }
            }

            pub fn input_with_builder<A>(&self, builder: A) -> Input<'input, A> {
                Input {
                    inner: self.inner.input_with_builder(&super::__RECURSA_GRAMMAR, builder),
                }
            }
        }

        pub fn lex(source: &str) -> LexResult<'_> {
            let scanned = #facade::__private::lex(
                source,
                __RECURSA_LEX_RULES,
                #lexical_error_kind,
            );
            let records = #facade::__private::lex_records(&scanned).to_vec();
            LexResult { inner: __recursa_finalize(source, records) }
        }

        }

        pub use __recursa_lexical::{
            GrammarInfo, LexBuildError, LexBuilder, LexBumpError, LexContext, LexEditError,
            LexEditor, LexError, LexErrors, LexRecord, LexResult, Token, Tokens, grammar_info, lex,
        };

        #[doc(hidden)]
        pub(crate) mod __recursa_content_tokens {
            #(#token_items)*
        }

        #visit_events

        macro_rules! __recursa_root_handshake {
            #(#capability_arms)*
            (#release, #protocol, $missing_capability:ident) => {
                compile_error!("error[RCA9005]: generated grammar requires a missing Recursa facade capability");
            };
            ($other_release:ident, $other_protocol:ident, $other_capability:ident) => {
                compile_error!("error[RCA9003]: Recursa release/private-protocol mismatch");
            };
        }

        macro_rules! __recursa_generation_handshake {
            (#generation) => {};
            ($other:ident) => {
                compile_error!("error[RCA9004]: root and Node selected different Generations");
            };
        }

        macro_rules! __recursa_load_node {
            #(#loader_arms)*
            ($other:ident, $other_anchor:literal) => {
                compile_error!("error[RCA9002]: selected Generation has no body for this Node");
            };
        }
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn typed_visit_node_variant(plan: &EmissionPlan, node: &NodeParsePlan) -> Option<Ident> {
    (node.publicly_materialized
        && node
            .generics
            .params
            .iter()
            .all(|parameter| matches!(parameter, GenericParam::Lifetime(_))))
    .then(|| visit_variant_ident(plan, &node.semantic_key))
}

fn typed_visit_token_binding(plan: &EmissionPlan, token: usize) -> Option<&TokenBindingPlan> {
    plan.token_bindings
        .iter()
        .find(|binding| binding.token == token && binding.publicly_materialized)
}

fn visit_variant_ident(plan: &EmissionPlan, semantic_key: &str) -> Ident {
    let anchor = plan
        .module_anchor
        .segments
        .last()
        .expect("a module anchor is nonempty")
        .ident
        .unraw()
        .to_string();
    format_ident!("{}", visit_variant_name(&anchor, semantic_key))
}

fn visit_variant_name(anchor: &str, semantic_key: &str) -> String {
    let segments = semantic_key.split("::").collect::<Vec<_>>();
    let anchor_index = segments
        .iter()
        .position(|segment| *segment == anchor)
        .map_or(0, |index| index + 1);
    let relative = semantic_key
        .split("::")
        .skip(anchor_index)
        .collect::<Vec<_>>()
        .join("::");
    visit_nested_variant(&relative)
}

fn visit_nested_variant(relative: &str) -> String {
    let suffix = relative
        .split("::")
        .map(|segment| format!("{}_{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    format!("__RecursaTarget_{suffix}")
}

fn render_visit_events(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let visit = plan
        .root_configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
        || plan
            .nodes
            .iter()
            .flat_map(|node| node.configured_traits.iter())
            .chain(
                plan.tokens
                    .iter()
                    .flat_map(|token| token.configured_traits.iter()),
            )
            .any(|recipe| *recipe == ConfiguredTraitRecipe::Visit);
    let visit_mut = plan
        .root_configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
        || plan
            .nodes
            .iter()
            .flat_map(|node| node.configured_traits.iter())
            .chain(
                plan.tokens
                    .iter()
                    .flat_map(|token| token.configured_traits.iter()),
            )
            .any(|recipe| *recipe == ConfiguredTraitRecipe::VisitMut);
    let node_variants = plan.nodes.iter().filter_map(|node| {
        let variant = typed_visit_node_variant(plan, node)?;
        let mut path = syn::parse_str::<syn::Path>(&format!("crate::{}", node.semantic_key))
            .expect("a resolved semantic key is a Rust path");
        let lifetimes = node
            .generics
            .params
            .iter()
            .filter(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
            .map(|_| quote!('__recursa_input))
            .collect::<Vec<_>>();
        if !lifetimes.is_empty() {
            path.segments
                .last_mut()
                .expect("nonempty semantic path")
                .arguments = PathArguments::AngleBracketed(syn::parse_quote!(<#(#lifetimes),*>));
        }
        let ty = quote!(#path);
        Some(quote!(#variant(&'__recursa_visit #ty),))
    });
    let token_variants = plan.tokens.iter().enumerate().filter_map(|(index, token)| {
        let binding = typed_visit_token_binding(plan, index)?;
        let variant = visit_variant_ident(plan, &binding.semantic_key);
        let canonical = &token.ident;
        Some(quote! {
            #variant(
                &'__recursa_visit
                    #module_anchor::__recursa_content_tokens::#canonical<'__recursa_input>,
            ),
        })
    });
    let node_variants_mut = plan.nodes.iter().filter_map(|node| {
        let variant = typed_visit_node_variant(plan, node)?;
        let mut path = syn::parse_str::<syn::Path>(&format!("crate::{}", node.semantic_key))
            .expect("a resolved semantic key is a Rust path");
        let lifetimes = node
            .generics
            .params
            .iter()
            .filter(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
            .map(|_| quote!('__recursa_input))
            .collect::<Vec<_>>();
        if !lifetimes.is_empty() {
            path.segments
                .last_mut()
                .expect("nonempty semantic path")
                .arguments = PathArguments::AngleBracketed(syn::parse_quote!(<#(#lifetimes),*>));
        }
        let ty = quote!(#path);
        Some(quote!(#variant(&'__recursa_visit mut #ty),))
    });
    let token_variants_mut = plan.tokens.iter().enumerate().filter_map(|(index, token)| {
        let binding = typed_visit_token_binding(plan, index)?;
        let variant = visit_variant_ident(plan, &binding.semantic_key);
        let canonical = &token.ident;
        Some(quote! {
            #variant(
                &'__recursa_visit mut
                    #module_anchor::__recursa_content_tokens::#canonical<'__recursa_input>,
            ),
        })
    });
    let visit_event = visit.then(|| {
        quote! {
            #[doc(hidden)]
            pub struct __RecursaVisitEvents<'__recursa_input>(
                ::core::marker::PhantomData<
                    fn(&'__recursa_input ()) -> &'__recursa_input (),
                >,
            );

            impl<'__recursa_input> #facade::__private::VisitEventFamily
                for __RecursaVisitEvents<'__recursa_input>
            {
                type Event<'__recursa_visit> =
                    __RecursaVisitEvent<'__recursa_visit, '__recursa_input>
                where
                    Self: '__recursa_visit;
            }

            #[doc(hidden)]
            #[allow(non_camel_case_types, private_interfaces)]
            pub enum __RecursaVisitEvent<'__recursa_visit, '__recursa_input>
            where
                '__recursa_input: '__recursa_visit,
            {
                #(#node_variants)*
                #(#token_variants)*
                Opaque(
                    &'__recursa_visit dyn #facade::__private::VisitNode,
                    ::core::marker::PhantomData<&'__recursa_input ()>,
                ),
            }
        }
    });
    let visit_mut_event = visit_mut.then(|| {
        quote! {
            #[doc(hidden)]
            pub struct __RecursaVisitEventsMut<'__recursa_input>(
                ::core::marker::PhantomData<
                    fn(&'__recursa_input ()) -> &'__recursa_input (),
                >,
            );

            impl<'__recursa_input> #facade::__private::VisitEventFamilyMut
                for __RecursaVisitEventsMut<'__recursa_input>
            {
                type EventMut<'__recursa_visit> =
                    __RecursaVisitEventMut<'__recursa_visit, '__recursa_input>
                where
                    Self: '__recursa_visit;
            }

            #[doc(hidden)]
            #[allow(non_camel_case_types, private_interfaces)]
            pub enum __RecursaVisitEventMut<'__recursa_visit, '__recursa_input>
            where
                '__recursa_input: '__recursa_visit,
            {
                #(#node_variants_mut)*
                #(#token_variants_mut)*
                Opaque(
                    &'__recursa_visit mut dyn #facade::__private::VisitNode,
                    ::core::marker::PhantomData<&'__recursa_input ()>,
                ),
            }
        }
    });
    quote!(#visit_event #visit_mut_event)
}

fn render_token(
    plan: &EmissionPlan,
    token: &TokenParsePlan,
    index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = &token.ident;
    let view = sibling_ident(ident, "View");
    let kind_variant = &token.kind_variant;
    let stores_kind = token.accepted_kind_variants.len() > 1;
    let accepted_kind_variants = &token.accepted_kind_variants;
    let kind_field = stores_kind.then(|| quote!(kind: #module_anchor::TokenKind,));
    let new_kind = stores_kind.then(|| quote!(kind: #module_anchor::TokenKind::#kind_variant,));
    let with_kind = stores_kind.then(|| {
        quote! {
            pub fn with_kind(
                kind: #module_anchor::TokenKind,
                text: impl Into<Box<str>>,
            ) -> Result<Self, #module_anchor::TokenKind> {
                match kind {
                    #(#module_anchor::TokenKind::#accepted_kind_variants)|* => Ok(Self {
                        text: #facade::__private::ContentText::owned(text.into()),
                        kind,
                    }),
                    rejected => Err(rejected),
                }
            }
        }
    });
    let kind_method = if stores_kind {
        quote! {
            pub const fn kind(&self) -> #module_anchor::TokenKind { self.kind }
        }
    } else {
        quote! {
            pub const fn kind(&self) -> #module_anchor::TokenKind {
                #module_anchor::TokenKind::#kind_variant
            }
        }
    };
    let parsed_kind = stores_kind.then(|| {
        let typed_kind_arms = accepted_kind_variants.iter().map(|variant| {
            let index = plan
                .kind_variants
                .iter()
                .position(|candidate| candidate == variant)
                .expect("an accepted token kind must have an emitted variant")
                as u16;
            quote!(#index => #module_anchor::TokenKind::#variant,)
        });
        quote! {
            kind: match record.kind {
                #(#typed_kind_arms)*
                _ => unreachable!("the parser admitted an unexpected token kind"),
            },
        }
    });
    let expected = format_ident!("__RECURSA_TOKEN_START_{index}");
    let capture = token.capture.render(facade);
    let standard_derives = token
        .configured_traits
        .iter()
        .filter_map(|recipe| match recipe {
            ConfiguredTraitRecipe::Debug => Some(quote!(Debug)),
            ConfiguredTraitRecipe::Clone => Some(quote!(Clone)),
            ConfiguredTraitRecipe::Copy => Some(quote!(Copy)),
            ConfiguredTraitRecipe::PartialEq => Some(quote!(PartialEq)),
            ConfiguredTraitRecipe::Eq => Some(quote!(Eq)),
            ConfiguredTraitRecipe::PartialOrd => Some(quote!(PartialOrd)),
            ConfiguredTraitRecipe::Ord => Some(quote!(Ord)),
            ConfiguredTraitRecipe::Hash => Some(quote!(Hash)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let standard_derive =
        (!standard_derives.is_empty()).then(|| quote!(#[derive(#(#standard_derives),*)]));
    let visit_event = quote!(#module_anchor::__RecursaVisitEvent);
    let visit_event_mut = quote!(#module_anchor::__RecursaVisitEventMut);
    let visit_events = quote!(#module_anchor::__RecursaVisitEvents);
    let visit_events_mut = quote!(#module_anchor::__RecursaVisitEventsMut);
    let visit_variant = typed_visit_token_binding(plan, index)
        .map(|binding| visit_variant_ident(plan, &binding.semantic_key));
    let enter_event = visit_variant.as_ref().map_or_else(
        || quote!(#visit_event::Opaque(self, ::core::marker::PhantomData)),
        |variant| quote!(#visit_event::#variant(self)),
    );
    let exit_event = enter_event.clone();
    let enter_event_mut = visit_variant.as_ref().map_or_else(
        || quote!(#visit_event_mut::Opaque(self, ::core::marker::PhantomData)),
        |variant| quote!(#visit_event_mut::#variant(self)),
    );
    let exit_event_mut = enter_event_mut.clone();
    let visit_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
        .then(|| {
            quote! {
                impl<'input> #facade::Visit for #ident<'input> {
                    type Events = #visit_events<'input>;

                    fn visit<V>(
                        &self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitor<Self::Events>,
                    {
                        match visitor.total_enter(#enter_event) {
                            ::core::ops::ControlFlow::Continue(())
                            | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit(#exit_event)
                    }
                }
            }
        });
    let visit_mut_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
        .then(|| {
            quote! {
                impl<'input> #facade::VisitMut for #ident<'input> {
                    type EventsMut = #visit_events_mut<'input>;

                    fn visit_mut<V>(
                        &mut self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitorMut<Self::EventsMut>,
                    {
                        match visitor.total_enter_mut(#enter_event_mut) {
                            ::core::ops::ControlFlow::Continue(())
                            | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit_mut(#exit_event_mut)
                    }
                }
            }
        });
    let pretty_impl = plan.pretty.tokens[index].map(|behavior| {
        let max_width = plan.pretty_max_width;
        let indent = plan.pretty_indent;
        let value = match behavior {
            PrettyTokenBehavior::MatchedOrStoredText => quote! {
                let matched = context.matched_text();
                context.token(matched.as_deref().unwrap_or_else(|| self.text()))
            },
        };
        quote! {
            impl<'input> #facade::Pretty for #ident<'input> {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.configure_defaults(#max_width, #indent);
                    #value
                }
            }

            impl<'input> #facade::Pretty for #view<'_, 'input> {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.with_cursor(self.cursor, |context| {
                        #facade::Pretty::pretty(self.value, context)
                    })
                }
            }
        }
    });
    let arbitrary_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Arbitrary)
        .then(|| {
            let spelling = &token.arbitrary_spelling;
            quote! {
                __recursa_if_arbitrary! {
                impl<'arbitrary, 'input> #facade::__private::arbitrary::Arbitrary<'arbitrary>
                    for #ident<'input>
                {
                    fn arbitrary(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        let _ = input;
                        Ok(Self::new(#spelling))
                    }
                }
                }
            }
        });
    let serialize_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Serialize)
        .then(|| {
            quote! {
                __recursa_if_serde! {
                    impl<'input> #facade::__private::serde::Serialize for #ident<'input> {
                        fn serialize<S: #facade::__private::serde::Serializer>(
                            &self,
                            serializer: S,
                        ) -> Result<S::Ok, S::Error> {
                            serializer.serialize_str(self.text())
                        }
                    }
                }
            }
        });
    let deserialize_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Deserialize)
        .then(|| {
            quote! {
                __recursa_if_serde! {
                    impl<'de, 'input> #facade::__private::serde::Deserialize<'de> for #ident<'input> {
                        fn deserialize<D: #facade::__private::serde::Deserializer<'de>>(
                            deserializer: D,
                        ) -> Result<Self, D::Error> {
                            let text = <String as #facade::__private::serde::Deserialize>::deserialize(deserializer)?;
                            Ok(Self::new(text))
                        }
                    }
                }
            }
        });
    quote! {
        #[doc(hidden)]
        #standard_derive
        pub struct #ident<'input> {
            text: #facade::__private::ContentText<'input>,
            #kind_field
        }

        impl<'input> #ident<'input> {
            pub fn new(text: impl Into<Box<str>>) -> Self {
                Self {
                    text: #facade::__private::ContentText::owned(text.into()),
                    #new_kind
                }
            }

            #with_kind
            pub fn text(&self) -> &str { self.text.as_str() }
            #kind_method
        }

        #[doc(hidden)]
        #[derive(Clone, Copy)]
        pub struct #view<'view, 'input>
        where 'input: 'view,
        {
            value: &'view #ident<'input>,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl<'input> #facade::__private::HasSpanSealed for #view<'_, 'input> {}
        impl<'input> #facade::HasSpan for #view<'_, 'input> {
            fn span(&self) -> Option<#facade::Span> { self.cursor.span() }
        }
        impl<'input> #facade::NodeView for #view<'_, 'input> {
            type Value = #ident<'input>;
            fn value(&self) -> &Self::Value { self.value }
        }
        impl<'input> #facade::__private::NodeViewFactory for #ident<'input> {
            type View<'view> = #view<'view, 'input> where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> { #view { value: self, cursor } }
        }
        impl<'input, A> #facade::__private::Parse<'input, A> for #ident<'input> {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy = #capture;
            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &#module_anchor::__RECURSA_GRAMMAR
            }
            fn root_follow() -> &'static #facade::__private::FollowSet {
                &#module_anchor::__RECURSA_EMPTY_FOLLOW
            }
            fn parse(
                context: &mut #facade::__private::ParseContext<'_, 'input, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                let Some(record) = context.peek(0) else {
                    return Err(context.unexpected(&#module_anchor::#expected));
                };
                if !#module_anchor::#expected.contains(record.kind) {
                    return Err(context.unexpected(&#module_anchor::#expected));
                }
                context.advance();
                let text = context.matched_text(record)?;
                context.finish(Self { text, #parsed_kind }, record.span)
            }
        }
        impl<'input, A> #facade::__private::ParseSealed<#module_anchor::Input<'input, A>>
            for #ident<'input>
        {}
        impl<'input, A> #facade::Parse<#module_anchor::Input<'input, A>> for #ident<'input> {
            type Output = #facade::Parsed<'input, Self>;
            fn parse(
                input: &mut #module_anchor::Input<'input, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }

        #visit_impl
        #visit_mut_impl
        #pretty_impl
        #arbitrary_impl
        #serialize_impl
        #deserialize_impl
    }
}

fn render_token_binding(
    plan: &EmissionPlan,
    binding: &TokenBindingPlan,
) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    let visibility = &binding.visibility;
    let ident = &binding.ident;
    let view = sibling_ident(ident, "View");
    let canonical = &plan.tokens[binding.token].ident;
    let canonical_view = sibling_ident(canonical, "View");
    let token_documentation = syn::LitStr::new(
        "A content-bearing token. Parsed values retain their exact matched input text; synthesized values own their text. Fixed tokens are syntax-only grammar elements: they shape parsing but store no value and have no generated token type.",
        proc_macro2::Span::call_site(),
    );
    quote! {
        #[doc = #token_documentation]
        #visibility type #ident<'input> =
            #module_anchor::__recursa_content_tokens::#canonical<'input>;
        #[doc = "A provenance-aware view of this generated content-bearing token."]
        #visibility type #view<'view, 'input> =
            #module_anchor::__recursa_content_tokens::#canonical_view<'view, 'input>;
    }
}

fn render_node(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let node_ident = &node.ident;
    let visibility = &node.visibility;
    let view_ident = sibling_ident(node_ident, "View");
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let generic_arguments = generic_arguments(generics);
    let mut view_generics = generics.clone();
    view_generics.params.insert(0, syn::parse_quote!('view));
    let (view_impl_generics, view_type_generics, view_where_clause) =
        view_generics.split_for_impl();
    let view_application = quote!(#view_ident<'view #(, #generic_arguments)*>);
    let input_lifetime = &node.input_lifetime;
    let inherent_method_generics = if generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let metadata = render_metadata(plan, node, node_index);
    let accessors = render_accessors(plan, node);
    let span_elements = render_span_elements(node, node_index);
    let parse_implementations = node
        .parse_applications
        .iter()
        .map(|application| render_node_parse_application(plan, node, application, instrumentation))
        .collect::<Vec<_>>();
    let token_bindings = if plan.nodes[..node_index]
        .iter()
        .any(|candidate| candidate.module == node.module)
    {
        Vec::new()
    } else {
        plan.token_bindings
            .iter()
            .filter(|binding| binding.module == node.module)
            .map(|binding| render_token_binding(plan, binding))
            .collect::<Vec<_>>()
    };
    let configured_traits = render_node_configured_traits(plan, node, node_index);
    #[cfg(feature = "diagrams")]
    let diagram_documentation = plan.diagrams.as_ref().map(|diagrams| {
        let diagram = &diagrams.nodes[node_index];
        syn::LitStr::new(
            &render_node_diagram_document(diagram),
            proc_macro2::Span::call_site(),
        )
    });
    #[cfg(feature = "diagrams")]
    let diagram_attribute = diagram_documentation
        .as_ref()
        .map(|documentation| quote!(#[doc = #documentation]));
    #[cfg(not(feature = "diagrams"))]
    let diagram_attribute = proc_macro2::TokenStream::new();
    let view_pretty = plan.pretty.nodes[node_index].is_some().then(|| {
        let mut pretty_generics = view_generics.clone();
        pretty_generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#node_ident #type_generics: #facade::Pretty));
        let (pretty_impl_generics, pretty_type_generics, pretty_where_clause) =
            pretty_generics.split_for_impl();
        quote! {
            impl #pretty_impl_generics #facade::Pretty
                for #view_ident #pretty_type_generics #pretty_where_clause
            {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.with_cursor(self.cursor, |context| {
                        #facade::Pretty::pretty(self.value, context)
                    })
                }
            }
        }
    });

    quote! {
        __recursa_generation_handshake!(#generation);

        #(#token_bindings)*

        #metadata

        #[derive(Clone, Copy)]
        #visibility struct #view_ident #view_generics {
            value: &'view #node_ident #type_generics,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl #view_impl_generics #view_ident #view_type_generics #view_where_clause {
            pub fn span_elements(self) -> #facade::Elements<'view> {
                #span_elements
            }

            #accessors
        }

        impl #view_impl_generics #facade::__private::HasSpanSealed
            for #view_ident #view_type_generics #view_where_clause
        {}

        impl #view_impl_generics #facade::HasSpan
            for #view_ident #view_type_generics #view_where_clause
        {
            fn span(&self) -> Option<#facade::Span> { self.cursor.span() }
        }

        impl #view_impl_generics #facade::NodeView
            for #view_ident #view_type_generics #view_where_clause
        {
            type Value = #node_ident #type_generics;
            fn value(&self) -> &Self::Value { self.value }
            fn span_elements(&self) -> #facade::Elements<'_> { #span_elements }
        }

        #view_pretty

        impl #impl_generics #facade::__private::NodeViewFactory for #node_ident #type_generics
            #where_clause
        {
            type View<'view> = #view_application where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> { #view_ident { value: self, cursor } }
        }

        #(#parse_implementations)*

        #configured_traits

        #diagram_attribute
        #[allow(dead_code)]
        impl #impl_generics #node_ident #type_generics #where_clause {
            pub fn parse #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::Parse<
                    #module_anchor::Input<#input_lifetime, A>,
                    Output = #facade::Parsed<#input_lifetime, Self>,
                >,
            {
                <Self as #facade::Parse<#module_anchor::Input<#input_lifetime, A>>>::parse(input)
            }
            pub fn parse_spanned #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::__private::Parse<#input_lifetime, A>,
            {
                #facade::__private::parse_spanned(&mut input.inner)
            }
        }
    }
}

fn render_node_parse_application(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = &application.input_lifetime;
    let ty = &application.ty;
    let mut parse_generics = Generics::default();
    let mut helper_generics = Generics::default();
    if input_lifetime.ident != "static" {
        let lifetime = input_lifetime.clone();
        parse_generics.params.push(syn::parse_quote!(#lifetime));
        helper_generics.params.push(syn::parse_quote!(#lifetime));
    }
    parse_generics.params.push(syn::parse_quote!(A));
    let (parse_impl_generics, _, parse_where_clause) = parse_generics.split_for_impl();
    let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
    let node_capture = node.capture.render(facade);
    let mut application_node = node.clone();
    application_node.body = application.body.clone();
    let planned_parse_body = render_node_parse_body(
        plan,
        &application_node,
        application.parse_index,
        instrumentation,
    );
    let (parse_body, pratt_helper) = if matches!(&application.body, NodeBodyPlan::Pratt(_)) {
        (
            quote!(Self::__recursa_parse_bp(context, 0)),
            quote! {
                impl #helper_impl_generics #ty #helper_where_clause {
                    fn __recursa_parse_bp<A>(
                        context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
                        min_bp: u8,
                    ) -> Result<
                        #facade::__private::ParsedNode<Self>,
                        #facade::ParseError,
                    > {
                        #planned_parse_body
                    }
                }
            },
        )
    } else {
        (planned_parse_body, quote!())
    };

    quote! {
        #pratt_helper

        impl #parse_impl_generics #facade::__private::Parse<#input_lifetime, A>
            for #ty #parse_where_clause
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy = #node_capture;
            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &#module_anchor::__RECURSA_GRAMMAR
            }
            fn root_follow() -> &'static #facade::__private::FollowSet {
                &#module_anchor::__RECURSA_EMPTY_FOLLOW
            }
            fn parse(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                #parse_body
            }
        }

        impl #parse_impl_generics
            #facade::__private::ParseSealed<#module_anchor::Input<#input_lifetime, A>>
            for #ty #parse_where_clause
        {}

        impl #parse_impl_generics #facade::Parse<#module_anchor::Input<#input_lifetime, A>>
            for #ty #parse_where_clause
        {
            type Output = #facade::Parsed<#input_lifetime, Self>;
            fn parse(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }
    }
}

fn render_node_configured_traits(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let mut implementations = Vec::new();
    for recipe in [
        ConfiguredTraitRecipe::Debug,
        ConfiguredTraitRecipe::Clone,
        ConfiguredTraitRecipe::Copy,
        ConfiguredTraitRecipe::PartialEq,
        ConfiguredTraitRecipe::Eq,
        ConfiguredTraitRecipe::PartialOrd,
        ConfiguredTraitRecipe::Ord,
        ConfiguredTraitRecipe::Hash,
    ] {
        if node.configured_traits.contains(&recipe) {
            implementations.push(render_node_standard_trait(plan, node, recipe));
        }
    }
    if node.configured_traits.iter().any(|recipe| {
        matches!(
            recipe,
            ConfiguredTraitRecipe::Serialize | ConfiguredTraitRecipe::Deserialize
        )
    }) {
        implementations.push(render_node_serde(plan, node));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Arbitrary)
    {
        implementations.push(render_node_arbitrary(plan, node));
    }
    if plan.pretty.nodes[node_index].is_some() {
        implementations.push(render_node_pretty(plan, node, node_index));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
    {
        implementations.push(render_node_visit(plan, node, false));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
    {
        implementations.push(render_node_visit(plan, node, true));
    }
    quote!(#(#implementations)*)
}

fn trait_impl_parts(
    facade: &Ident,
    node: &NodeParsePlan,
    bound: proc_macro2::TokenStream,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let mut generics = node.generics.clone();
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    (
        quote!(#impl_generics),
        quote!(#type_generics),
        quote!(#where_clause),
    )
}

fn render_node_standard_trait(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    recipe: ConfiguredTraitRecipe,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let trait_path = match recipe {
        ConfiguredTraitRecipe::Debug => quote!(::core::fmt::Debug),
        ConfiguredTraitRecipe::Clone => quote!(::core::clone::Clone),
        ConfiguredTraitRecipe::Copy => quote!(::core::marker::Copy),
        ConfiguredTraitRecipe::PartialEq => quote!(::core::cmp::PartialEq),
        ConfiguredTraitRecipe::Eq => quote!(::core::cmp::Eq),
        ConfiguredTraitRecipe::PartialOrd => quote!(::core::cmp::PartialOrd),
        ConfiguredTraitRecipe::Ord => quote!(::core::cmp::Ord),
        ConfiguredTraitRecipe::Hash => quote!(::core::hash::Hash),
        _ => unreachable!("grammar-aware recipes are rendered separately"),
    };
    let (impl_generics, type_generics, where_clause) =
        trait_impl_parts(facade, node, trait_path.clone());
    match recipe {
        ConfiguredTraitRecipe::Debug => {
            let body = render_debug_body(node);
            quote! {
                impl #impl_generics ::core::fmt::Debug for #node_ident #type_generics #where_clause {
                    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        #body
                    }
                }
            }
        }
        ConfiguredTraitRecipe::Clone => {
            let body = render_clone_body(node);
            quote! {
                impl #impl_generics ::core::clone::Clone for #node_ident #type_generics #where_clause {
                    fn clone(&self) -> Self { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Copy => quote! {
            impl #impl_generics ::core::marker::Copy for #node_ident #type_generics #where_clause {}
        },
        ConfiguredTraitRecipe::PartialEq => {
            let body = render_partial_eq_body(node);
            quote! {
                impl #impl_generics ::core::cmp::PartialEq for #node_ident #type_generics #where_clause {
                    fn eq(&self, other: &Self) -> bool { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Eq => quote! {
            impl #impl_generics ::core::cmp::Eq for #node_ident #type_generics #where_clause {}
        },
        ConfiguredTraitRecipe::PartialOrd => {
            let body = render_order_body(node, true);
            quote! {
                impl #impl_generics ::core::cmp::PartialOrd for #node_ident #type_generics #where_clause {
                    fn partial_cmp(&self, other: &Self) -> Option<::core::cmp::Ordering> { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Ord => {
            let body = render_order_body(node, false);
            quote! {
                impl #impl_generics ::core::cmp::Ord for #node_ident #type_generics #where_clause {
                    fn cmp(&self, other: &Self) -> ::core::cmp::Ordering { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Hash => {
            let body = render_hash_body(node);
            quote! {
                impl #impl_generics ::core::hash::Hash for #node_ident #type_generics #where_clause {
                    fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) { #body }
                }
            }
        }
        _ => unreachable!("grammar-aware recipes are rendered separately"),
    }
}

fn semantic_fields(elements: &[ElementParsePlan]) -> Vec<&FieldPlan> {
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, .. } => Some(field),
            ElementParseKind::Fixed { .. } => None,
        })
        .collect()
}

fn trait_variants(body: &NodeBodyPlan) -> Vec<VariantParsePlan> {
    match body {
        NodeBodyPlan::Enum { variants, .. } => variants.clone(),
        NodeBodyPlan::Pratt(pratt) => {
            let mut variants = pratt
                .nud
                .iter()
                .map(|branch| (branch.source_index, branch.variant.clone()))
                .chain(pratt.led.iter().map(|branch| {
                    let mut variant = branch.variant.clone();
                    variant.elements.insert(0, branch.lhs.clone());
                    (branch.source_index, variant)
                }))
                .collect::<Vec<_>>();
            variants.sort_by_key(|(source_index, _)| *source_index);
            variants.into_iter().map(|(_, variant)| variant).collect()
        }
        NodeBodyPlan::Struct { .. } => unreachable!("struct Nodes do not have variants"),
    }
}

fn render_debug_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    let node_name = node.ident.unraw().to_string();
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().map(|field| {
                        let ident = field.ident.as_ref().expect("named field");
                        let name = ident.unraw().to_string();
                        let access = field_access(quote!(self), Some(ident), field.index);
                        quote!(debug.field(#name, &#access);)
                    });
                    quote! {
                        let mut debug = formatter.debug_struct(#node_name);
                        #(#entries)*
                        debug.finish()
                    }
                }
                ResolvedFieldStyle::Unnamed => {
                    let entries = fields.iter().map(|field| {
                        let access = field_access(quote!(self), None, field.index);
                        quote!(debug.field(&#access);)
                    });
                    quote! {
                        let mut debug = formatter.debug_tuple(#node_name);
                        #(#entries)*
                        debug.finish()
                    }
                }
                ResolvedFieldStyle::Unit => quote!(formatter.write_str(#node_name)),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let name = variant.ident.unraw().to_string();
                let fields = semantic_fields(&variant.elements);
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "debug");
                match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(&bindings).map(|(field, binding)| {
                            let field_name = field.ident.as_ref().expect("named field").unraw().to_string();
                            quote!(debug.field(#field_name, #binding);)
                        });
                        quote!(#pattern => { let mut debug = formatter.debug_struct(#name); #(#entries)* debug.finish() })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let entries = bindings.iter().map(|binding| quote!(debug.field(#binding);));
                        quote!(#pattern => { let mut debug = formatter.debug_tuple(#name); #(#entries)* debug.finish() })
                    }
                    ResolvedFieldStyle::Unit => quote!(#pattern => formatter.write_str(#name)),
                }
            });
            quote!(match self { #(#arms),* })
        }
    }
}

fn variant_bound_pattern(
    node: &Ident,
    variant: &VariantParsePlan,
    prefix: &str,
) -> (proc_macro2::TokenStream, Vec<Ident>) {
    let fields = semantic_fields(&variant.elements);
    let bindings = fields
        .iter()
        .map(|field| format_ident!("__recursa_{prefix}_{}", field.index))
        .collect::<Vec<_>>();
    let variant_ident = &variant.ident;
    let pattern = match variant.style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().zip(&bindings).map(|(field, binding)| {
                let ident = field.ident.as_ref().expect("named field");
                quote!(#ident: #binding)
            });
            quote!(#node::#variant_ident { #(#entries),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let field_count = fields
                .iter()
                .map(|field| field.index + 1)
                .max()
                .unwrap_or(0);
            let mut entries = (0..field_count).map(|_| quote!(_)).collect::<Vec<_>>();
            for (field, binding) in fields.iter().zip(&bindings) {
                entries[field.index] = quote!(#binding);
            }
            quote!(#node::#variant_ident(#(#entries),*))
        }
        ResolvedFieldStyle::Unit => quote!(#node::#variant_ident),
    };
    (pattern, bindings)
}

fn render_clone_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            let values = fields.iter().map(|field| {
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                quote!(::core::clone::Clone::clone(&#access))
            });
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().zip(values).map(|(field, value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => quote!(Self(#(#values),*)),
                ResolvedFieldStyle::Unit => quote!(Self),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let fields = semantic_fields(&variant.elements);
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "clone");
                let values = bindings
                    .iter()
                    .map(|binding| quote!(::core::clone::Clone::clone(#binding)));
                let variant_ident = &variant.ident;
                let value = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(values).map(|(field, value)| {
                            let ident = field.ident.as_ref().expect("named field");
                            quote!(#ident: #value)
                        });
                        quote!(Self::#variant_ident { #(#entries),* })
                    }
                    ResolvedFieldStyle::Unnamed => quote!(Self::#variant_ident(#(#values),*)),
                    ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
                };
                quote!(#pattern => #value)
            });
            quote!(match self { #(#arms),* })
        }
    }
}

fn render_partial_eq_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let comparisons = semantic_fields(elements).into_iter().map(|field| {
                let left = field_access(quote!(self), field.ident.as_ref(), field.index);
                let right = field_access(quote!(other), field.ident.as_ref(), field.index);
                quote!(::core::cmp::PartialEq::eq(&#left, &#right))
            });
            quote!(true #(&& #comparisons)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let (left_pattern, left) = variant_bound_pattern(&node.ident, variant, "eq_left");
                let (right_pattern, right) =
                    variant_bound_pattern(&node.ident, variant, "eq_right");
                let comparisons = left
                    .iter()
                    .zip(right)
                    .map(|(left, right)| quote!(::core::cmp::PartialEq::eq(#left, #right)));
                quote!((#left_pattern, #right_pattern) => true #(&& #comparisons)*)
            });
            quote!(match (self, other) { #(#arms,)* _ => false })
        }
    }
}

fn render_order_body(node: &NodeParsePlan, partial: bool) -> proc_macro2::TokenStream {
    let compare_fields = |left: Vec<proc_macro2::TokenStream>,
                          right: Vec<proc_macro2::TokenStream>| {
        let comparisons = left.into_iter().zip(right).map(|(left, right)| {
            if partial {
                quote! {
                    match ::core::cmp::PartialOrd::partial_cmp(#left, #right) {
                        Some(::core::cmp::Ordering::Equal) => {}
                        ordering => return ordering,
                    }
                }
            } else {
                quote! {
                    match ::core::cmp::Ord::cmp(#left, #right) {
                        ::core::cmp::Ordering::Equal => {}
                        ordering => return ordering,
                    }
                }
            }
        });
        if partial {
            quote!({ #(#comparisons)* Some(::core::cmp::Ordering::Equal) })
        } else {
            quote!({ #(#comparisons)* ::core::cmp::Ordering::Equal })
        }
    };
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let fields = semantic_fields(elements);
            let left = fields
                .iter()
                .map(|field| {
                    let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                    quote!(&#access)
                })
                .collect();
            let right = fields
                .iter()
                .map(|field| {
                    let access = field_access(quote!(other), field.ident.as_ref(), field.index);
                    quote!(&#access)
                })
                .collect();
            compare_fields(left, right)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let same_variant = variants.iter().map(|variant| {
                let (left_pattern, left) = variant_bound_pattern(&node.ident, variant, "ord_left");
                let (right_pattern, right) =
                    variant_bound_pattern(&node.ident, variant, "ord_right");
                let left = left.iter().map(|binding| quote!(#binding)).collect();
                let right = right.iter().map(|binding| quote!(#binding)).collect();
                let body = compare_fields(left, right);
                quote!((#left_pattern, #right_pattern) => #body)
            });
            let left_indices = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, _) = variant_bound_pattern(&node.ident, variant, "ord_variant_left");
                quote!(#pattern => #index)
            });
            let right_indices = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, _) = variant_bound_pattern(&node.ident, variant, "ord_variant_right");
                quote!(#pattern => #index)
            });
            let differing = if partial {
                quote! {
                    let left = match self { #(#left_indices),* };
                    let right = match other { #(#right_indices),* };
                    Some(::core::cmp::Ord::cmp(&left, &right))
                }
            } else {
                quote! {
                    let left = match self { #(#left_indices),* };
                    let right = match other { #(#right_indices),* };
                    ::core::cmp::Ord::cmp(&left, &right)
                }
            };
            quote!(match (self, other) { #(#same_variant,)* _ => { #differing } })
        }
    }
}

fn render_hash_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let hashes = semantic_fields(elements).into_iter().map(|field| {
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                quote!(::core::hash::Hash::hash(&#access, state);)
            });
            quote!(#(#hashes)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "hash");
                let hashes = bindings
                    .iter()
                    .map(|binding| quote!(::core::hash::Hash::hash(#binding, state);));
                quote!(#pattern => { ::core::hash::Hash::hash(&#index, state); #(#hashes)* })
            });
            quote!(match self { #(#arms),* })
        }
    }
}

fn render_node_visit(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    mutable: bool,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let node_ident = &node.ident;
    let mut generics = node.generics.clone();
    let event_input = if node
        .generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        let input_lifetime = &node.input_lifetime;
        quote!(#input_lifetime)
    } else {
        quote!('static)
    };
    let bound = if mutable {
        quote!(#facade::VisitMut<
            EventsMut = #module_anchor::__RecursaVisitEventsMut<#event_input>
        >)
    } else {
        quote!(#facade::Visit<
            Events = #module_anchor::__RecursaVisitEvents<#event_input>
        >)
    };
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let children = render_visit_children(facade, node, mutable);
    let variant = typed_visit_node_variant(plan, node);
    if mutable {
        let event = quote!(#module_anchor::__RecursaVisitEventMut);
        let events = quote!(#module_anchor::__RecursaVisitEventsMut);
        let enter = variant.as_ref().map_or_else(
            || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
            |variant| quote!(#event::#variant(self)),
        );
        let exit = enter.clone();
        quote! {
            impl #impl_generics #facade::VisitMut for #node_ident #type_generics #where_clause {
                type EventsMut = #events<#event_input>;

                fn visit_mut<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<Self::EventsMut>,
                {
                    match visitor.total_enter_mut(#enter) {
                        ::core::ops::ControlFlow::Continue(()) => { #children }
                        ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit_mut(#exit)
                }
            }
        }
    } else {
        let event = quote!(#module_anchor::__RecursaVisitEvent);
        let events = quote!(#module_anchor::__RecursaVisitEvents);
        let enter = variant.as_ref().map_or_else(
            || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
            |variant| quote!(#event::#variant(self)),
        );
        let exit = enter.clone();
        quote! {
            impl #impl_generics #facade::Visit for #node_ident #type_generics #where_clause {
                type Events = #events<#event_input>;

                fn visit<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<Self::Events>,
                {
                    match visitor.total_enter(#enter) {
                        ::core::ops::ControlFlow::Continue(()) => { #children }
                        ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit(#exit)
                }
            }
        }
    }
}

fn render_pratt_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    pratt: &PrattParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Pratt behavior");
    let (impl_generics, type_generics, where_clause) =
        trait_impl_parts(facade, node, quote!(#facade::Pretty));
    let variants = trait_variants(&node.body);
    let arms = variants
        .iter()
        .zip(&behavior.variants)
        .map(|(variant, behavior)| {
            let position = pratt
                .nud
                .iter()
                .find(|branch| branch.variant.ident == variant.ident)
                .map(|branch| branch.position)
                .or_else(|| {
                    pratt
                        .led
                        .iter()
                        .find(|branch| branch.variant.ident == variant.ident)
                        .map(|branch| branch.position)
                })
                .expect("every Pratt variant has a frozen position");
            let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty_pratt");
            let mut binding_index = 0_usize;
            let mut semantic_index = 0_usize;
            let rendered = variant
            .elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(|(index, (element, behavior))| match (&element.kind, behavior) {
                (
                    ElementParseKind::Fixed { .. },
                    PrettyElementBehavior::Fixed { spelling, spacing },
                ) => {
                    let infix = matches!(position, PrattPosition::Infix { .. });
                    let prefix = matches!(position, PrattPosition::Prefix { .. });
                    let fixed = render_pretty_fixed(plan, index, spelling, *spacing);
                    if infix && *spacing == PrettySpacing::Auto {
                        quote! {
                            context.soft_break(" ")?;
                            #fixed
                            context.soft_break(" ")?;
                        }
                    } else if prefix && *spacing == PrettySpacing::Auto {
                        let fallback = if plan.text_equivalence
                            == TextEquivalence::AsciiInsensitive
                            && is_word_shaped(spelling)
                        {
                            spelling.to_ascii_uppercase()
                        } else {
                            spelling.clone()
                        };
                        quote! {
                            context.with_child(#index, |context| {
                                let matched = context.matched_text();
                                context.opening_token(matched.as_deref().unwrap_or(#fallback))
                            })?;
                        }
                    } else {
                        fixed
                    }
                }
                (
                    ElementParseKind::Semantic {
                        terminal,
                        wrappers,
                        container,
                        ..
                    },
                    PrettyElementBehavior::Semantic { attachment, .. },
                ) => {
                    assert!(
                        matches!(container.as_ref(), PlannedContainer::Direct { .. })
                            && wrappers
                                .iter()
                                .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed)),
                        "validated Pratt operands are direct and may only use Box storage wrappers",
                    );
                    let binding = &bindings[binding_index];
                    binding_index += 1;
                    let current_semantic = semantic_index;
                    semantic_index += 1;
                    let recursive =
                        terminal_path_ident(terminal).is_some_and(|ident| ident == node_ident);
                    let semantic = if recursive {
                        let (minimum, next) = match position {
                            PrattPosition::Atom => (quote!(0u16), quote!(None)),
                            PrattPosition::Prefix { bp } => {
                                (quote!(#bp as u16), quote!(__recursa_inner_next))
                            }
                            PrattPosition::Postfix { bp } => {
                                (quote!(__recursa_inner_min), quote!(Some(#bp as u16)))
                            }
                            PrattPosition::Infix { lbp, .. } if current_semantic == 0 => {
                                (quote!(__recursa_inner_min), quote!(Some(#lbp as u16)))
                            }
                            PrattPosition::Infix { rbp, .. } => {
                                (quote!(#rbp as u16), quote!(__recursa_inner_next))
                            }
                        };
                        quote!((#binding).__recursa_pretty_pratt(context, #minimum, #next))
                    } else {
                        quote!(#facade::Pretty::pretty(#binding, context))
                    };
                    if let Some(attachment) = attachment {
                        let unit = render_pretty_attachment_unit(attachment, semantic);
                        quote! {
                            context.with_child(#index, |context| {
                                context.with_child(0, |context| { #unit Ok(()) })
                            })?;
                        }
                    } else {
                        quote! {
                            context.with_child(#index, |context| {
                                context.with_child(0, |context| { #semantic })
                            })?;
                        }
                    }
                }
                (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. }) => {
                    unreachable!("validated Pretty Pratt element behavior")
                }
            })
            .collect::<Vec<_>>();
            let rendered = render_pretty_sequence(behavior, &rendered);
            let (entry, exit) = match position {
                PrattPosition::Atom => (None, 256_u16),
                PrattPosition::Prefix { bp } => (None, u16::from(bp)),
                PrattPosition::Infix { lbp, rbp } => (Some(u16::from(lbp)), u16::from(rbp)),
                PrattPosition::Postfix { bp } => (Some(u16::from(bp)), 256_u16),
            };
            let entry_check = entry.map_or_else(|| quote!(false), |entry| quote!(#entry < min_bp));
            quote! {
                #pattern => {
                    let __recursa_parenthesize = #entry_check
                        || next_lbp.is_some_and(|next| next >= #exit);
                    if __recursa_parenthesize {
                        context.opening_token("(")?;
                    }
                    let __recursa_inner_min = if __recursa_parenthesize { 0 } else { min_bp };
                    let __recursa_inner_next = if __recursa_parenthesize { None } else { next_lbp };
                    #rendered
                    if __recursa_parenthesize {
                        context.tight_token(")")?;
                    }
                    Ok(())
                }
            }
        });
    quote! {
        impl #impl_generics #node_ident #type_generics #where_clause {
            fn __recursa_pretty_pratt(
                &self,
                context: &mut #facade::PrettyContext,
                min_bp: u16,
                next_lbp: Option<u16>,
            ) -> ::core::fmt::Result {
                match self { #(#arms),* }
            }
        }

        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.group(|context| self.__recursa_pretty_pratt(context, 0, None))
            }
        }
    }
}

fn terminal_path_ident(terminal: &Type) -> Option<&Ident> {
    let Type::Path(path) = terminal else {
        return None;
    };
    path.path.segments.last().map(|segment| &segment.ident)
}

fn render_node_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let behavior = plan.pretty.nodes[node_index]
        .as_ref()
        .expect("validated Pretty Node behavior");
    let (impl_generics, type_generics, where_clause) =
        trait_impl_parts(facade, node, quote!(#facade::Pretty));
    let elements = |elements: &[ElementParsePlan],
                    behavior: &[PrettyElementBehavior],
                    base: proc_macro2::TokenStream| {
        let rendered = elements
            .iter()
            .zip(behavior)
            .enumerate()
            .map(
                |(index, (element, behavior))| match (&element.kind, behavior) {
                    (
                        ElementParseKind::Fixed { .. },
                        PrettyElementBehavior::Fixed { spelling, spacing },
                    ) => render_pretty_fixed(plan, index, spelling, *spacing),
                    (
                        ElementParseKind::Semantic {
                            field,
                            wrappers,
                            container,
                            ..
                        },
                        PrettyElementBehavior::Semantic {
                            repeated,
                            separator,
                            attachment,
                        },
                    ) => {
                        let value = field_access(base.clone(), field.ident.as_ref(), field.index);
                        if let Some(attachment) = attachment {
                            return render_pretty_attached_semantic(
                                facade, index, value, false, wrappers, container, attachment,
                            );
                        }
                        if *repeated {
                            separator.as_ref().map_or_else(
                                || {
                                    quote! {
                                        context.with_child(#index, |context| {
                                            #facade::Pretty::pretty(&#value, context)
                                        })?;
                                    }
                                },
                                |separator| {
                                    quote! {
                                        context.with_child(#index, |context| {
                                            context.with_repetition(#separator, false, |context| {
                                                #facade::Pretty::pretty(&#value, context)
                                            })
                                        })?;
                                    }
                                },
                            )
                        } else {
                            quote! {
                                context.with_child(#index, |context| {
                                    context.with_child(0, |context| {
                                        #facade::Pretty::pretty(&#value, context)
                                    })
                                })?;
                            }
                        }
                    }
                    (ElementParseKind::Fixed { .. }, PrettyElementBehavior::Semantic { .. })
                    | (ElementParseKind::Semantic { .. }, PrettyElementBehavior::Fixed { .. }) => {
                        unreachable!("validated Pretty element behavior")
                    }
                },
            )
            .collect::<Vec<_>>();
        render_pretty_sequence(behavior, &rendered)
    };
    let body = match &node.body {
        NodeBodyPlan::Struct {
            elements: fields, ..
        } => {
            let rendered = elements(fields, &behavior.variants[0], quote!(self));
            quote!({ #rendered Ok(()) })
        }
        NodeBodyPlan::Pratt(pratt) => {
            return render_pratt_node_pretty(plan, node, node_index, pratt);
        }
        NodeBodyPlan::Enum { .. } => {
            let variants = trait_variants(&node.body);
            let arms = variants
                .iter()
                .zip(&behavior.variants)
                .map(|(variant, behavior)| {
                    let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "pretty");
                    let mut binding_index = 0;
                    let rendered = variant.elements.iter().zip(behavior).enumerate().map(
                        |(index, (element, behavior))| match (&element.kind, behavior) {
                            (
                                ElementParseKind::Fixed { .. },
                                PrettyElementBehavior::Fixed { spelling, spacing },
                            ) => render_pretty_fixed(plan, index, spelling, *spacing),
                            (
                                ElementParseKind::Semantic {
                                    wrappers,
                                    container,
                                    ..
                                },
                                PrettyElementBehavior::Semantic {
                                    repeated,
                                    separator,
                                    attachment,
                                },
                            ) => {
                                let binding = &bindings[binding_index];
                                binding_index += 1;
                                if let Some(attachment) = attachment {
                                    return render_pretty_attached_semantic(
                                        facade,
                                        index,
                                        quote!(#binding),
                                        true,
                                        wrappers,
                                        container,
                                        attachment,
                                    );
                                }
                                if *repeated {
                                    separator.as_ref().map_or_else(
                                        || quote! {
                                            context.with_child(#index, |context| {
                                                #facade::Pretty::pretty(#binding, context)
                                            })?;
                                        },
                                        |separator| quote! {
                                            context.with_child(#index, |context| {
                                                context.with_repetition(#separator, false, |context| {
                                                    #facade::Pretty::pretty(#binding, context)
                                                })
                                            })?;
                                        },
                                    )
                                } else {
                                    quote! {
                                        context.with_child(#index, |context| {
                                            context.with_child(0, |context| {
                                                #facade::Pretty::pretty(#binding, context)
                                            })
                                        })?;
                                    }
                                }
                            }
                            (
                                ElementParseKind::Fixed { .. },
                                PrettyElementBehavior::Semantic { .. },
                            )
                            | (
                                ElementParseKind::Semantic { .. },
                                PrettyElementBehavior::Fixed { .. },
                            ) => unreachable!("validated Pretty element behavior"),
                        },
                    ).collect::<Vec<_>>();
                    let rendered = render_pretty_sequence(behavior, &rendered);
                    quote!(#pattern => { #rendered Ok(()) })
                });
            quote!(match self { #(#arms),* })
        }
    };
    quote! {
        impl #impl_generics #facade::Pretty for #node_ident #type_generics #where_clause {
            fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.group(|context| { #body })
            }
        }
    }
}

fn render_pretty_fixed(
    plan: &EmissionPlan,
    index: usize,
    spelling: &str,
    spacing: PrettySpacing,
) -> proc_macro2::TokenStream {
    let method = match spacing {
        PrettySpacing::Tight => format_ident!("tight_sequence_token"),
        PrettySpacing::Before => format_ident!("before_token"),
        PrettySpacing::After => format_ident!("after_token"),
        PrettySpacing::Around => format_ident!("token"),
        PrettySpacing::Auto if matches!(spelling, "(" | "[" | "{") => {
            format_ident!("opening_token")
        }
        PrettySpacing::Auto if is_word_shaped(spelling) => format_ident!("token"),
        PrettySpacing::Auto => format_ident!("tight_token"),
    };
    let fallback =
        if plan.text_equivalence == TextEquivalence::AsciiInsensitive && is_word_shaped(spelling) {
            spelling.to_ascii_uppercase()
        } else {
            spelling.to_owned()
        };
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#fallback))
        })?;
    }
}

fn render_pretty_sequence(
    behavior: &[PrettyElementBehavior],
    rendered: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    let delimiter = behavior.first().and_then(|first| {
        let PrettyElementBehavior::Fixed { spelling, .. } = first else {
            return None;
        };
        delimiter_close(spelling).filter(|close| {
            behavior.last().is_some_and(|last| {
                matches!(last, PrettyElementBehavior::Fixed { spelling, .. } if spelling == close)
            })
        })
    });
    if delimiter.is_some() && rendered.len() >= 2 {
        let open = &rendered[0];
        let middle = &rendered[1..rendered.len() - 1];
        let close = &rendered[rendered.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#middle)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
        }
    } else {
        quote!(#(#rendered)*)
    }
}

fn render_pretty_attached_fixed(
    index: usize,
    fixed: &PrettyAttachedFixedBehavior,
) -> proc_macro2::TokenStream {
    let method = match fixed.spacing {
        PrettySpacing::Tight => format_ident!("tight_sequence_token"),
        PrettySpacing::Before => format_ident!("before_token"),
        PrettySpacing::After => format_ident!("after_token"),
        PrettySpacing::Around => format_ident!("token"),
        PrettySpacing::Auto if matches!(fixed.spelling.as_str(), "(" | "[" | "{") => {
            format_ident!("opening_token")
        }
        PrettySpacing::Auto if is_word_shaped(&fixed.spelling) => format_ident!("token"),
        PrettySpacing::Auto => format_ident!("tight_token"),
    };
    let spelling = &fixed.spelling;
    quote! {
        context.with_child(#index, |context| {
            let matched = context.matched_text();
            context.#method(matched.as_deref().unwrap_or(#spelling))
        })?;
    }
}

fn render_pretty_attachment_unit(
    attachment: &PrettyAttachmentBehavior,
    semantic_body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let before = attachment
        .before
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(index, fixed))
        .collect::<Vec<_>>();
    let semantic_index = before.len();
    let semantic = quote! {
        context.with_child(#semantic_index, |context| { #semantic_body })?;
    };
    let after = attachment
        .after
        .iter()
        .enumerate()
        .map(|(index, fixed)| render_pretty_attached_fixed(semantic_index + 1 + index, fixed))
        .collect::<Vec<_>>();
    let delimited = attachment
        .before
        .first()
        .and_then(|open| delimiter_close(&open.spelling))
        .is_some_and(|close| {
            attachment
                .after
                .last()
                .is_some_and(|fixed| fixed.spelling == close)
        });
    if delimited {
        let open = &before[0];
        let leading = &before[1..];
        let close = after
            .last()
            .expect("delimited attachment has a close token");
        let trailing = &after[..after.len() - 1];
        quote! {
            #open
            context.indented(|context| {
                context.soft_break("")?;
                #(#leading)*
                #semantic
                #(#trailing)*
                Ok(())
            })?;
            context.soft_break("")?;
            #close
        }
    } else {
        quote!(#(#before)* #semantic #(#after)*)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_pretty_attached_semantic(
    facade: &Ident,
    element_index: usize,
    value: proc_macro2::TokenStream,
    value_is_reference: bool,
    wrappers: &[FieldWrapper],
    container: &PlannedContainer,
    attachment: &PrettyAttachmentBehavior,
) -> proc_macro2::TokenStream {
    let render_unit = |terminal: proc_macro2::TokenStream| {
        render_pretty_attachment_unit(
            attachment,
            quote!(#facade::Pretty::pretty(#terminal, context)),
        )
    };

    if matches!(container, PlannedContainer::Repeated { .. }) {
        let repeat_index = wrappers
            .iter()
            .position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            })
            .expect("repeated attached field has a repetition wrapper");
        let outer = &wrappers[..repeat_index];
        let inner = &wrappers[repeat_index + 1..];
        let item = render_terminal_reference(quote!(item), inner, true);
        let unit = render_unit(item);
        let render_collection = |collection: proc_macro2::TokenStream| {
            let separator = attachment.separator.as_ref().map(|separator| {
                quote! {
                    if item_index + 1 < item_count {
                        context.tight_token(#separator)?;
                        context.soft_break(" ")?;
                    }
                }
            });
            quote! {
                let item_count = (#collection).len();
                for (item_index, item) in (#collection).iter().enumerate() {
                    context.with_child(item_index, |context| { #unit Ok(()) })?;
                    #separator
                }
            }
        };
        let outer_optional = outer
            .iter()
            .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
        let body = if outer_optional {
            let collection = render_optional_view_reference(value, outer, value_is_reference);
            let rendered = render_collection(quote!(collection));
            quote! {
                if let Some(collection) = #collection {
                    #rendered
                }
            }
        } else {
            let collection = render_terminal_reference(value, outer, value_is_reference);
            render_collection(collection)
        };
        quote! {
            context.with_child(#element_index, |context| {
                #body
                Ok(())
            })?;
        }
    } else if wrappers
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional))
    {
        let terminal = render_optional_view_reference(value, wrappers, value_is_reference);
        let unit = render_unit(quote!(terminal));
        quote! {
            if let Some(terminal) = #terminal {
                context.with_child(#element_index, |context| {
                    context.with_child(0, |context| { #unit Ok(()) })
                })?;
            }
        }
    } else {
        let terminal = render_terminal_reference(value, wrappers, value_is_reference);
        let unit = render_unit(terminal);
        quote! {
            context.with_child(#element_index, |context| {
                context.with_child(0, |context| { #unit Ok(()) })
            })?;
        }
    }
}

fn delimiter_close(open: &str) -> Option<&'static str> {
    match open {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        _ => None,
    }
}

fn render_node_arbitrary(plan: &EmissionPlan, node: &NodeParsePlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let mut generics = node.generics.clone();
    generics.params.insert(0, syn::parse_quote!('arbitrary));
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #field_type: #facade::__private::arbitrary::Arbitrary<'arbitrary>
            ));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, type_generics, _) = node.generics.split_for_impl();
    let arbitrary_value = || quote!(#facade::__private::arbitrary::Arbitrary::arbitrary(input)?);
    let body = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            let values = fields.iter().map(|_| arbitrary_value());
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().zip(values).map(|(field, value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => quote!(Self(#(#values),*)),
                ResolvedFieldStyle::Unit => quote!(Self),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let fields = semantic_fields(&variant.elements);
                let values = fields.iter().map(|_| arbitrary_value());
                let variant_ident = &variant.ident;
                let value = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(values).map(|(field, value)| {
                            let ident = field.ident.as_ref().expect("named field");
                            quote!(#ident: #value)
                        });
                        quote!(Self::#variant_ident { #(#entries),* })
                    }
                    ResolvedFieldStyle::Unnamed => quote!(Self::#variant_ident(#(#values),*)),
                    ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
                };
                quote!(#index => #value)
            });
            let last = variants.len().saturating_sub(1);
            quote! {
                match input.int_in_range(0usize..=#last)? {
                    #(#arms,)*
                    _ => unreachable!("the arbitrary variant index is in range"),
                }
            }
        }
    };
    quote! {
        __recursa_if_arbitrary! {
        impl #impl_generics #facade::__private::arbitrary::Arbitrary<'arbitrary>
            for #node_ident #type_generics #where_clause
        {
            fn arbitrary(
                input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
            ) -> #facade::__private::arbitrary::Result<Self> {
                Ok(#body)
            }
        }
        }
    }
}

fn render_node_serde(plan: &EmissionPlan, node: &NodeParsePlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let remote = node_ident.unraw().to_string();
    let helper = format_ident!(
        "__RecursaSerde_{}",
        stable_hash(node.semantic_key.as_bytes())
    );
    let serde_crate = format!("{}::__private::serde", facade.unraw());
    let mut derives = Vec::new();
    let serialize = node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Serialize);
    let deserialize = node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Deserialize);
    if serialize {
        derives.push(quote!(#facade::__private::serde::Serialize));
    }
    if deserialize {
        derives.push(quote!(#facade::__private::serde::Deserialize));
    }
    let generics = &node.generics;
    let helper_body = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = elements.iter().filter_map(|element| {
                let ElementParseKind::Semantic {
                    field,
                    terminal,
                    wrappers,
                    ..
                } = &element.kind
                else {
                    return None;
                };
                Some((field, render_wrapped_type(facade, terminal, wrappers)))
            });
            match style {
                ResolvedFieldStyle::Named => {
                    let fields = fields.map(|(field, ty)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #ty)
                    });
                    quote!(struct #helper #generics { #(#fields),* })
                }
                ResolvedFieldStyle::Unnamed => {
                    let fields = fields.map(|(_, ty)| quote!(#ty));
                    quote!(struct #helper #generics (#(#fields),*);)
                }
                ResolvedFieldStyle::Unit => quote!(struct #helper #generics;),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let variants = variants.iter().map(|variant| {
                let ident = &variant.ident;
                let fields = variant.elements.iter().filter_map(|element| {
                    let ElementParseKind::Semantic {
                        field,
                        terminal,
                        wrappers,
                        ..
                    } = &element.kind
                    else {
                        return None;
                    };
                    Some((field, render_wrapped_type(facade, terminal, wrappers)))
                });
                match variant.style {
                    ResolvedFieldStyle::Named => {
                        let fields = fields.map(|(field, ty)| {
                            let field_ident = field.ident.as_ref().expect("named field");
                            quote!(#field_ident: #ty)
                        });
                        quote!(#ident { #(#fields),* })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let fields = fields.map(|(_, ty)| quote!(#ty));
                        quote!(#ident(#(#fields),*))
                    }
                    ResolvedFieldStyle::Unit => quote!(#ident),
                }
            });
            quote!(enum #helper #generics { #(#variants),* })
        }
    };
    let helper_definition = quote! {
        #[derive(#(#derives),*)]
        #[serde(remote = #remote, crate = #serde_crate)]
        #[allow(non_camel_case_types)]
        #helper_body
    };
    let serialize_impl = serialize.then(|| {
        let (impl_generics, type_generics, where_clause) =
            trait_impl_parts(facade, node, quote!(#facade::__private::serde::Serialize));
        quote! {
            impl #impl_generics #facade::__private::serde::Serialize
                for #node_ident #type_generics #where_clause
            {
                fn serialize<S: #facade::__private::serde::Serializer>(
                    &self,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    #helper::serialize(self, serializer)
                }
            }
        }
    });
    let deserialize_impl = deserialize.then(|| {
        let mut impl_generics = node.generics.clone();
        impl_generics
            .params
            .insert(0, syn::parse_quote!('__recursa_de));
        for field_type in semantic_field_types(facade, node) {
            impl_generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(
                    #field_type: #facade::__private::serde::Deserialize<'__recursa_de>
                ));
        }
        let (impl_generics, _, where_clause) = impl_generics.split_for_impl();
        let (_, type_generics, _) = node.generics.split_for_impl();
        quote! {
            impl #impl_generics #facade::__private::serde::Deserialize<'__recursa_de>
                for #node_ident #type_generics #where_clause
            {
                fn deserialize<D: #facade::__private::serde::Deserializer<'__recursa_de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    #helper::deserialize(deserializer)
                }
            }
        }
    });
    quote! {
        __recursa_if_serde! {
        #helper_definition
        #serialize_impl
        #deserialize_impl
        }
    }
}

fn semantic_field_types(facade: &Ident, node: &NodeParsePlan) -> Vec<Type> {
    let elements = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements.clone(),
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .into_iter()
            .flat_map(|variant| variant.elements)
            .collect(),
    };
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic {
                terminal, wrappers, ..
            } if !is_recursive_terminal(node, terminal) => {
                Some(render_wrapped_type(facade, terminal, wrappers))
            }
            ElementParseKind::Semantic { .. } => None,
            ElementParseKind::Fixed { .. } => None,
        })
        .collect()
}

fn is_recursive_terminal(node: &NodeParsePlan, terminal: &Type) -> bool {
    let Type::Path(terminal) = terminal else {
        return false;
    };
    let segments = terminal
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>();
    let segments = segments
        .strip_prefix(&["crate".to_owned()])
        .unwrap_or(&segments);
    segments.join("::") == node.semantic_key
}

fn render_visit_children(
    facade: &Ident,
    node: &NodeParsePlan,
    mutable: bool,
) -> proc_macro2::TokenStream {
    let visit = |value: proc_macro2::TokenStream| {
        if mutable {
            quote!(#facade::VisitMut::visit_mut(#value, visitor)?;)
        } else {
            quote!(#facade::Visit::visit(#value, visitor)?;)
        }
    };
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let fields = elements.iter().filter_map(|element| {
                let ElementParseKind::Semantic { field, .. } = &element.kind else {
                    return None;
                };
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                Some(visit(if mutable {
                    quote!(&mut #access)
                } else {
                    quote!(&#access)
                }))
            });
            quote!(#(#fields)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let node_ident = &node.ident;
            let arms = variants.iter().map(|variant| {
                let semantic = variant
                    .elements
                    .iter()
                    .filter_map(|element| match &element.kind {
                        ElementParseKind::Semantic { field, .. } => Some(field),
                        ElementParseKind::Fixed { .. } => None,
                    })
                    .collect::<Vec<_>>();
                let bindings = semantic
                    .iter()
                    .map(|field| format_ident!("__recursa_visit_field_{}", field.index))
                    .collect::<Vec<_>>();
                let variant_ident = &variant.ident;
                let pattern = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let fields = semantic.iter().zip(&bindings).map(|(field, binding)| {
                            let ident = field
                                .ident
                                .as_ref()
                                .expect("a named variant field has an identifier");
                            quote!(#ident: #binding)
                        });
                        quote!(#node_ident::#variant_ident { #(#fields),* })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let field_count = semantic
                            .iter()
                            .map(|field| field.index + 1)
                            .max()
                            .unwrap_or(0);
                        let mut fields = (0..field_count).map(|_| quote!(_)).collect::<Vec<_>>();
                        for (field, binding) in semantic.iter().zip(&bindings) {
                            fields[field.index] = quote!(#binding);
                        }
                        quote!(#node_ident::#variant_ident(#(#fields),*))
                    }
                    ResolvedFieldStyle::Unit => quote!(#node_ident::#variant_ident),
                };
                let visits = bindings.into_iter().map(|binding| visit(quote!(#binding)));
                quote!(#pattern => { #(#visits)* })
            });
            quote!(match self { #(#arms),* })
        }
    }
}

fn render_metadata(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let name = metadata_static(node_index, None);
            let entries = elements
                .iter()
                .map(|element| render_metadata_entry(facade, element));
            quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let definitions = variants.iter().enumerate().map(|(index, variant)| {
                let name = metadata_static(node_index, Some(index));
                let entries = variant
                    .elements
                    .iter()
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            quote!(#(#definitions)*)
        }
        NodeBodyPlan::Pratt(pratt) => {
            let nud = pratt.nud.iter().enumerate().map(|(index, branch)| {
                let name = metadata_static(node_index, Some(index));
                let entries = branch
                    .variant
                    .elements
                    .iter()
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            let led_offset = pratt.nud.len();
            let led = pratt.led.iter().enumerate().map(|(index, branch)| {
                let name = metadata_static(node_index, Some(led_offset + index));
                let entries = std::iter::once(&branch.lhs)
                    .chain(branch.variant.elements.iter())
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            quote!(#(#nud)* #(#led)*)
        }
    }
}

fn render_metadata_entry(facade: &Ident, element: &ElementParsePlan) -> proc_macro2::TokenStream {
    let name = &element.metadata_name;
    let role = match element.kind {
        ElementParseKind::Fixed { .. } => quote!(#facade::ElementRole::FixedToken),
        ElementParseKind::Semantic { .. } => quote!(#facade::ElementRole::SemanticField),
    };
    quote!(#facade::__private::element_metadata(Some(#name), #role))
}

fn render_span_elements(node: &NodeParsePlan, node_index: usize) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { .. } => {
            let metadata = metadata_static(node_index, None);
            quote!(self.cursor.elements(#metadata))
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let node_ident = &node.ident;
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let variant_ident = &variant.ident;
                let pattern = variant_pattern(node_ident, variant_ident, variant.style);
                let metadata = metadata_static(node_index, Some(index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            quote!(match self.value { #(#arms)* })
        }
        NodeBodyPlan::Pratt(pratt) => {
            let node_ident = &node.ident;
            let nud = pratt.nud.iter().enumerate().map(|(index, branch)| {
                let variant = &branch.variant;
                let pattern = variant_pattern(node_ident, &variant.ident, variant.style);
                let metadata = metadata_static(node_index, Some(index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            let led_offset = pratt.nud.len();
            let led = pratt.led.iter().enumerate().map(|(index, branch)| {
                let variant = &branch.variant;
                let pattern = variant_pattern(node_ident, &variant.ident, variant.style);
                let metadata = metadata_static(node_index, Some(led_offset + index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            quote!(match self.value { #(#nud)* #(#led)* })
        }
    }
}

fn render_accessors(plan: &EmissionPlan, node: &NodeParsePlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let methods = elements
                .iter()
                .enumerate()
                .filter_map(|(index, element)| render_accessor(facade, element, index, None));
            quote!(#(#methods)*)
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let methods = variants.iter().flat_map(|variant| {
                variant
                    .elements
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            quote!(#(#methods)*)
        }
        NodeBodyPlan::Pratt(pratt) => {
            let nud = pratt.nud.iter().flat_map(|branch| {
                let variant = &branch.variant;
                variant
                    .elements
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            let led = pratt.led.iter().flat_map(|branch| {
                let variant = &branch.variant;
                std::iter::once(&branch.lhs)
                    .chain(variant.elements.iter())
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            quote!(#(#nud)* #(#led)*)
        }
    }
}

fn render_accessor(
    facade: &Ident,
    element: &ElementParsePlan,
    element_index: usize,
    variant: Option<(&Ident, &Ident, ResolvedFieldStyle)>,
) -> Option<proc_macro2::TokenStream> {
    match &element.kind {
        ElementParseKind::Fixed { accessor, .. } => {
            let method = accessor;
            let active = variant.map(|(node, variant, style)| {
                let pattern = variant_pattern(node, variant, style);
                quote! {
                    if !matches!(self.value, #pattern) { return None; }
                }
            });
            Some(quote! {
                pub fn #method(&self) -> Option<#facade::Span> {
                    #active
                    self.cursor.child(#element_index).span()
                }
            })
        }
        ElementParseKind::Semantic {
            field,
            terminal,
            wrappers,
            attachment,
            ..
        } => {
            let accessor = field.accessor.as_ref()?;
            if let Some(repeat_index) = wrappers.iter().position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            }) {
                return Some(render_repeated_accessor(
                    facade,
                    accessor,
                    field,
                    terminal,
                    wrappers,
                    repeat_index,
                    element_index,
                    variant,
                    attachment.as_deref(),
                ));
            }
            let is_optional = wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
            let semantic_child = attachment
                .as_ref()
                .map(|attachment| attachment.before.len());
            let semantic_cursor = |cursor: proc_macro2::TokenStream| {
                semantic_child.map_or(cursor.clone(), |child| quote!((#cursor).child(#child)))
            };
            if let Some((node, variant_ident, style)) = variant {
                let binding = format_ident!("__recursa_field");
                let active_pattern = field_binding_pattern(
                    node,
                    variant_ident,
                    style,
                    field.ident.as_ref(),
                    field.index,
                    &binding,
                );
                let value = if is_optional {
                    let optional_reference =
                        render_optional_view_reference(quote!(#binding), wrappers, true);
                    let cursor =
                        semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                    quote! {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                #cursor,
                            )
                        })
                    }
                } else {
                    let reference = render_terminal_reference(quote!(#binding), wrappers, true);
                    let cursor =
                        semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                    quote! {
                        Some(#facade::__private::NodeViewFactory::view(
                            #reference,
                            #cursor,
                        ))
                    }
                };
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view>
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        match self.value {
                            #active_pattern => #value,
                            _ => None,
                        }
                    }
                })
            } else if is_optional {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let optional_reference = render_optional_view_reference(reference, wrappers, false);
                let cursor = semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view>
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                #cursor,
                            )
                        })
                    }
                })
            } else {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let reference = render_terminal_reference(reference, wrappers, false);
                let cursor = semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> impl #facade::NodeView<Value = #terminal> + 'view
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        #facade::__private::NodeViewFactory::view(
                            #reference,
                            #cursor,
                        )
                    }
                })
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_repeated_accessor(
    facade: &Ident,
    accessor: &Ident,
    field: &FieldPlan,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    repeat_index: usize,
    element_index: usize,
    variant: Option<(&Ident, &Ident, ResolvedFieldStyle)>,
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let outer = &wrappers[..repeat_index];
    let inner = &wrappers[repeat_index + 1..];
    let item_type = render_wrapped_type(facade, terminal, inner);
    let project = render_terminal_reference(quote!(value), inner, true);
    let cursor = quote!(self.cursor.child(#element_index));
    let repeated = |collection: proc_macro2::TokenStream| {
        attachment.map_or_else(
            || {
                quote! {
                    #facade::Repeated::new(
                        (#collection).as_slice(),
                        #cursor,
                        |value: &#item_type| -> &#terminal { #project },
                    )
                }
            },
            |attachment| {
                let semantic_child = attachment.before.len();
                quote! {
                    #facade::Repeated::new_attached(
                        (#collection).as_slice(),
                        #cursor,
                        |value: &#item_type| -> &#terminal { #project },
                        #semantic_child,
                    )
                }
            },
        )
    };
    let outer_optional = outer
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));

    if let Some((node, variant_ident, style)) = variant {
        let binding = format_ident!("__recursa_field");
        let active_pattern = field_binding_pattern(
            node,
            variant_ident,
            style,
            field.ident.as_ref(),
            field.index,
            &binding,
        );
        let active_value = if outer_optional {
            let collection = render_optional_view_reference(quote!(#binding), outer, true);
            let repeated = repeated(quote!(collection));
            quote!(#collection.map(|collection| #repeated))
        } else {
            let collection = render_terminal_reference(quote!(#binding), outer, true);
            let repeated = repeated(collection);
            quote!(Some(#repeated))
        };
        quote! {
            pub fn #accessor(
                &self,
            ) -> Option<#facade::Repeated<'view, #terminal, #item_type>>
            where
                #terminal: #facade::__private::NodeViewFactory,
            {
                match self.value {
                    #active_pattern => #active_value,
                    _ => None,
                }
            }
        }
    } else {
        let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
        if outer_optional {
            let collection = render_optional_view_reference(reference, outer, false);
            let repeated = repeated(quote!(collection));
            quote! {
                pub fn #accessor(
                    &self,
                ) -> Option<#facade::Repeated<'view, #terminal, #item_type>>
                where
                    #terminal: #facade::__private::NodeViewFactory,
                {
                    #collection.map(|collection| #repeated)
                }
            }
        } else {
            let collection = render_terminal_reference(reference, outer, false);
            let repeated = repeated(collection);
            quote! {
                pub fn #accessor(
                    &self,
                ) -> #facade::Repeated<'view, #terminal, #item_type>
                where
                    #terminal: #facade::__private::NodeViewFactory,
                {
                    #repeated
                }
            }
        }
    }
}

fn fixed_accessor_ident(metadata_name: &str) -> Result<Ident, String> {
    let direct = format!("{metadata_name}_span");
    emitted_ident(&direct).or_else(|_| emitted_ident(&format!("element_{direct}")))
}

fn render_node_parse_body(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            render_sequence_parse(plan, node_index, None, *style, elements, None, None)
        }
        NodeBodyPlan::Enum { variants, dispatch } => {
            let mut expected_index = 1_000_000;
            render_predictive_dispatch(
                plan,
                node_index,
                variants,
                &dispatch.root,
                0,
                &mut expected_index,
                instrumentation,
            )
        }
        NodeBodyPlan::Pratt(pratt) => render_pratt_parse(plan, node_index, pratt, instrumentation),
    }
}

fn render_predictive_dispatch(
    plan: &EmissionPlan,
    node_index: usize,
    variants: &[VariantParsePlan],
    node: &DispatchTrieNodePlan,
    depth: usize,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    instrumentation.frozen_dispatch_nodes_rendered += 1;
    if let Some(target) = node.target.filter(|target| !target.on_caller_follow) {
        debug_assert!(node.edges.is_empty());
        let variant = &variants[target.variant];
        return render_sequence_parse(
            plan,
            node_index,
            Some((target.variant, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            None,
        );
    }
    let facade = &plan.facade;
    let mut expected = ExpectedSet::empty(plan.kind_count);
    for edge in &node.edges {
        expected.union_assign(&edge.position.expected);
    }
    let expected_name = expected_static(node_index, *expected_index, "dispatch");
    *expected_index += 1;
    let expected_definition = render_expected_static(&expected_name, &expected, plan, facade);

    let target = node.target.map(|target| {
        let variant = &variants[target.variant];
        let body = render_sequence_parse(
            plan,
            node_index,
            Some((target.variant, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            None,
        );
        debug_assert!(target.on_caller_follow);
        quote! {
            if context.peek(#depth).is_none()
                || context
                    .peek(#depth)
                    .is_some_and(|record| context.follow().contains(record.kind))
            {
                return { #body };
            }
        }
    });

    let edges = node.edges.iter().map(|edge| {
        let position_name = expected_static(node_index, *expected_index, "position");
        *expected_index += 1;
        let position_definition =
            render_expected_static(&position_name, &edge.position.expected, plan, facade);
        let matches = render_dispatch_position_match(
            plan,
            &position_name,
            &edge.position.text_predicate,
            depth,
        );
        let next = render_predictive_dispatch(
            plan,
            node_index,
            variants,
            &edge.next,
            depth + 1,
            expected_index,
            instrumentation,
        );
        quote! {
            #position_definition
            if #matches {
                return { #next };
            }
        }
    });

    let failure = if node.target.is_some_and(|target| target.on_caller_follow) {
        quote! {
            let __recursa_dispatch_expected = #expected_name
                .union(context.follow())
                .expect("matching generated dispatch/FOLLOW widths");
            Err(context.unexpected_at(#depth, &__recursa_dispatch_expected))
        }
    } else {
        quote!(Err(context.unexpected_at(#depth, &#expected_name)))
    };

    quote! {
        #expected_definition
        #target
        #(#edges)*
        #failure
    }
}

fn render_dispatch_position_match(
    plan: &EmissionPlan,
    expected: &Ident,
    predicate: &Option<PlannedTextPredicate>,
    depth: usize,
) -> proc_macro2::TokenStream {
    let predicate = predicate.as_ref().map(|predicate| {
        let matches = match predicate {
            PlannedTextPredicate::Any => quote!(true),
            PlannedTextPredicate::Exact(exact) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!(text == #exact),
                TextEquivalence::AsciiInsensitive => quote!(text.eq_ignore_ascii_case(#exact)),
            },
            PlannedTextPredicate::OneOf(options) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!([#(#options),*].contains(&text)),
                TextEquivalence::AsciiInsensitive => {
                    quote!([#(#options),*].iter().any(|option| text.eq_ignore_ascii_case(option)))
                }
            },
            PlannedTextPredicate::NotIn(options) => match plan.text_equivalence {
                TextEquivalence::Sensitive => quote!(![#(#options),*].contains(&text)),
                TextEquivalence::AsciiInsensitive => {
                    quote!(![#(#options),*].iter().any(|option| text.eq_ignore_ascii_case(option)))
                }
            },
        };
        quote! {
            && context
                .source()
                .get(record.span.range())
                .is_some_and(|text| #matches)
        }
    });
    quote! {
        context.peek(#depth).is_some_and(|record| {
            #expected.contains(record.kind) #predicate
        })
    }
}

fn render_predictive_selection(
    plan: &EmissionPlan,
    node_index: usize,
    node: &DispatchTrieNodePlan,
    depth: usize,
    caller_follow: &proc_macro2::TokenStream,
    expected_index: &mut usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    instrumentation.frozen_dispatch_nodes_rendered += 1;
    let fallback = node.target.map_or_else(
        || quote!(None),
        |target| {
            let variant = target.variant;
            if target.on_caller_follow {
                quote! {
                    if context.peek(#depth).is_none()
                        || context
                            .peek(#depth)
                            .is_some_and(|record| #caller_follow.contains(record.kind))
                    {
                        Some(#variant)
                    } else {
                        None
                    }
                }
            } else {
                debug_assert!(node.edges.is_empty());
                quote!(Some(#variant))
            }
        },
    );
    node.edges.iter().rev().fold(fallback, |otherwise, edge| {
        let position_name = expected_static(node_index, *expected_index, "pratt_position");
        *expected_index += 1;
        let position_definition =
            render_expected_static(&position_name, &edge.position.expected, plan, &plan.facade);
        let matches = render_dispatch_position_match(
            plan,
            &position_name,
            &edge.position.text_predicate,
            depth,
        );
        let selected = render_predictive_selection(
            plan,
            node_index,
            &edge.next,
            depth + 1,
            caller_follow,
            expected_index,
            instrumentation,
        );
        quote! {{
            #position_definition
            if #matches { #selected } else { #otherwise }
        }}
    })
}

fn render_pratt_parse(
    plan: &EmissionPlan,
    node_index: usize,
    pratt: &PrattParsePlan,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let owner = quote!(Self);
    let expression_start = format_ident!("__recursa_expression_start");
    let mut expected_index = 2_000_000;
    let nud_follow_name = expected_static(node_index, 3_200_000, "pratt_nud_follow");
    let nud_follow_definition =
        render_expected_static(&nud_follow_name, &pratt.nud_selection_follow, plan, facade);
    let nud_follow_value = format_ident!("__recursa_pratt_nud_follow");
    let nud_selection = render_predictive_selection(
        plan,
        node_index,
        &pratt.nud_dispatch.root,
        0,
        &quote!(#nud_follow_value),
        &mut expected_index,
        instrumentation,
    );
    let nud_failure_name = expected_static(node_index, 3_100_000, "pratt_nud_failure");
    let (nud_failure_definition, nud_failure_value) =
        render_follow_value(plan, &nud_failure_name, &pratt.nud_failure);
    let nud_branches = pratt.nud.iter().enumerate().map(|(index, branch)| {
        let variant = &branch.variant;
        let body = render_sequence_parse(
            plan,
            node_index,
            Some((index, &variant.ident)),
            variant.style,
            &variant.elements,
            variant.capture,
            Some(&owner),
        );
        quote!(Some(#index) => { #body? })
    });

    if pratt.led.is_empty() {
        return quote! {
            #nud_failure_definition
            #nud_follow_definition
            let _ = min_bp;
            let #nud_follow_value = #nud_follow_name
                .union(context.follow())
                .expect("matching generated Pratt FOLLOW widths");
            let #expression_start = context.cursor();
            let parsed = match { #nud_selection } {
                #(#nud_branches,)*
                None => return Err(context.unexpected(&#nud_failure_value)),
                Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
            };
            let _ = #expression_start;
            Ok(parsed)
        };
    }

    let led_selection = render_predictive_selection(
        plan,
        node_index,
        &pratt
            .led_dispatch
            .as_ref()
            .expect("validated led plan has a frozen dispatch")
            .root,
        0,
        &quote!(context.follow()),
        &mut expected_index,
        instrumentation,
    );
    let guard = pratt
        .caller_follow_guard
        .as_ref()
        .expect("validated led plan has a caller-FOLLOW guard");
    let guard_definitions = guard.iter().enumerate().map(|(index, branch)| {
        let name = expected_static(node_index, 3_000_000 + index, "pratt_guard");
        render_expected_static(&name, &branch.expected, plan, &plan.facade)
    });
    let guard_additions = guard.iter().enumerate().map(|(index, branch)| {
        let name = expected_static(node_index, 3_000_000 + index, "pratt_guard");
        let bp = pratt_led_lbp(branch.position);
        quote! {
            if #bp >= min_bp {
                __recursa_pratt_guard = Some(match __recursa_pratt_guard {
                    Some(expected) => expected
                        .union(&#name)
                        .expect("matching generated Pratt expectation widths"),
                    None => #name.clone(),
                });
            }
        }
    });
    let led_offset = pratt.nud.len();
    let mut led_branches = Vec::new();
    for (index, branch) in pratt.led.iter().enumerate() {
        let metadata_index = led_offset + index;
        let lbp = pratt_led_lbp(branch.position);
        let body = render_pratt_led_parse(
            plan,
            node_index,
            metadata_index,
            &owner,
            branch,
            &expression_start,
        );
        led_branches.push(quote! {
            Some(#index) => {
                if #lbp < min_bp { break; }
                if context.next_is(context.follow()) {
                    return Err(context.pratt_ambiguity(
                        __recursa_pratt_guard
                            .as_ref()
                            .expect("an active extender contributes an expectation"),
                    ));
                }
                lhs = { #body };
                continue;
            }
        });
    }

    quote! {
        #nud_failure_definition
        #nud_follow_definition
        #(#guard_definitions)*
        let #nud_follow_value = #nud_follow_name
            .union(context.follow())
            .expect("matching generated Pratt FOLLOW widths");
        let #expression_start = context.cursor();
        let mut lhs = match { #nud_selection } {
            #(#nud_branches,)*
            None => return Err(context.unexpected(&#nud_failure_value)),
            Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
        };
        let mut __recursa_pratt_guard: Option<#facade::__private::FollowSet> = None;
        #(#guard_additions)*
        loop {
            match { #led_selection } {
                #(#led_branches)*
                None => break,
                Some(_) => unreachable!("frozen Pratt dispatch selected an unknown branch"),
            }
        }
        Ok(lhs)
    }
}

fn pratt_led_lbp(position: PrattPosition) -> u8 {
    match position {
        PrattPosition::Infix { lbp, .. } => lbp,
        PrattPosition::Postfix { bp } => bp,
        PrattPosition::Atom | PrattPosition::Prefix { .. } => {
            unreachable!("a Pratt led plan has a nud position")
        }
    }
}

fn render_pratt_led_parse(
    plan: &EmissionPlan,
    node_index: usize,
    variant_index: usize,
    owner: &proc_macro2::TokenStream,
    branch: &PrattLedParsePlan,
    expression_start: &Ident,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let variant = &branch.variant;
    let lhs_value = format_ident!("__recursa_lhs_value");
    let lhs_occurrence = format_ident!("__recursa_lhs_occurrence");
    let ElementParseKind::Semantic {
        field: lhs_field,
        wrappers: lhs_wrappers,
        capture: lhs_capture,
        ..
    } = &branch.lhs.kind
    else {
        unreachable!("a planned Pratt lhs is semantic")
    };
    let wrapped_lhs = render_wrapped_value(quote!(parsed_lhs), lhs_wrappers);
    let lhs_capture = lhs_capture.render(facade);
    let mut definitions = vec![quote! {
        let (parsed_lhs, parsed_lhs_occurrence) = lhs.into_parts();
        let parsed_lhs_span = context.consumed_span_from(#expression_start);
        let #lhs_occurrence = context.finish_group(
            #lhs_capture,
            parsed_lhs_span,
            [parsed_lhs_occurrence],
        )?;
        let #lhs_value = #wrapped_lhs;
    }];
    let mut values = vec![(lhs_field.clone(), lhs_value)];
    let mut occurrences = vec![lhs_occurrence];
    for (tail_index, element) in variant.elements.iter().enumerate() {
        let element_index = tail_index + 1;
        let value = format_ident!("__recursa_value_{element_index}");
        let occurrence = format_ident!("__recursa_occurrence_{element_index}");
        occurrences.push(occurrence.clone());
        let unique_index = variant_index * 1000 + element_index;
        match &element.kind {
            ElementParseKind::Fixed {
                expected, capture, ..
            } => {
                let expected_name = expected_static(node_index, unique_index, "fixed");
                let expected_definition =
                    render_expected_static(&expected_name, expected, plan, facade);
                let capture = capture.render(facade);
                definitions.push(quote! {
                    #expected_definition
                    let #occurrence = context.parse_fixed(&#expected_name, #capture)?;
                });
            }
            ElementParseKind::Semantic {
                field,
                terminal,
                wrappers,
                container,
                capture,
                child_capture,
                pratt_min_bp,
                attachment,
                ..
            } => {
                definitions.push(render_semantic_parse(
                    plan,
                    node_index,
                    unique_index,
                    &value,
                    &occurrence,
                    terminal,
                    wrappers,
                    container,
                    *capture,
                    *child_capture,
                    Some(owner),
                    *pratt_min_bp,
                    attachment.as_deref(),
                ));
                values.push((field.clone(), value));
            }
        }
    }
    let construction = render_construction(Some(&variant.ident), variant.style, &values);
    let finish = variant.capture.map_or_else(
        || quote!(context.finish_with_children(#construction, span, [#(#occurrences),*])),
        |capture| {
            let capture = capture.render(facade);
            quote!(context.finish_with_children_policy(
                #construction,
                span,
                #capture,
                [#(#occurrences),*],
            ))
        },
    );
    quote! {
        #(#definitions)*
        let span = context.consumed_span_from(#expression_start);
        #finish?
    }
}

fn render_sequence_parse(
    plan: &EmissionPlan,
    node_index: usize,
    variant: Option<(usize, &Ident)>,
    style: ResolvedFieldStyle,
    elements: &[ElementParsePlan],
    capture: Option<PlannedCapturePolicy>,
    pratt_owner: Option<&proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let start = format_ident!("__recursa_start");
    let mut definitions = Vec::new();
    let mut values = Vec::new();
    let mut occurrences = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let value = format_ident!("__recursa_value_{index}");
        let occurrence = format_ident!("__recursa_occurrence_{index}");
        occurrences.push(occurrence.clone());
        let unique_index = variant.map_or(index, |(variant, _)| variant * 1000 + index);
        match &element.kind {
            ElementParseKind::Fixed {
                expected, capture, ..
            } => {
                let expected_name = expected_static(node_index, unique_index, "fixed");
                let expected_definition =
                    render_expected_static(&expected_name, expected, plan, facade);
                let capture = capture.render(facade);
                definitions.push(quote! {
                    #expected_definition
                    let #occurrence = context.parse_fixed(&#expected_name, #capture)?;
                });
            }
            ElementParseKind::Semantic {
                field,
                terminal,
                wrappers,
                container,
                capture,
                child_capture,
                pratt_min_bp,
                attachment,
                ..
            } => {
                definitions.push(render_semantic_parse(
                    plan,
                    node_index,
                    unique_index,
                    &value,
                    &occurrence,
                    terminal,
                    wrappers,
                    container,
                    *capture,
                    *child_capture,
                    pratt_owner,
                    *pratt_min_bp,
                    attachment.as_deref(),
                ));
                values.push((field.clone(), value));
            }
        }
    }
    let target = variant.map(|(_, ident)| ident);
    let construction = render_construction(target, style, &values);
    let finish = capture.map_or_else(
        || quote!(context.finish_with_children(#construction, span, [#(#occurrences),*])),
        |capture| {
            let capture = capture.render(facade);
            quote!(context.finish_with_children_policy(
                #construction,
                span,
                #capture,
                [#(#occurrences),*],
            ))
        },
    );
    if elements.is_empty() {
        quote! {
            let _ = context;
            compile_error!("nullable empty Node bodies are not emitted by compound ticket #64");
        }
    } else {
        quote! {
            let #start = context.cursor();
            #(#definitions)*
            let span = context.consumed_span_from(#start);
            #finish
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_semantic_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    value: &Ident,
    occurrence: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    container: &PlannedContainer,
    capture: PlannedCapturePolicy,
    child_capture: PlannedCapturePolicy,
    pratt_owner: Option<&proc_macro2::TokenStream>,
    pratt_min_bp: Option<u8>,
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let capture = capture.render(facade);
    let child_capture = child_capture.render(facade);
    match container {
        PlannedContainer::Direct {
            child_follow,
            required_failure,
        } => {
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let failure_name = expected_static(node_index, unique_index, "required_failure");
            let (failure_definition, failure_value) =
                render_follow_value(plan, &failure_name, required_failure);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let direct_child = match (pratt_owner, pratt_min_bp) {
                (Some(owner), Some(min_bp)) => quote! {
                    context.parse_generated(&#follow_value, #child_capture, |context| {
                        #owner::__recursa_parse_bp(context, #min_bp)
                    })
                },
                _ => quote! {
                    context.parse_node::<#terminal>(&#follow_value, #child_capture)
                },
            };
            let (attachment_definitions, parse_child) = attachment.map_or_else(
                || (quote!(), direct_child),
                |attachment| {
                    render_attachment_child(
                        plan,
                        node_index,
                        unique_index,
                        terminal,
                        attachment,
                        &follow_value,
                        &child_capture,
                        pratt_owner,
                        pratt_min_bp,
                    )
                },
            );
            quote! {
                #follow_definition
                #failure_definition
                #attachment_definitions
                let element_start = context.cursor();
                let (parsed_value, child_occurrence) = match #parse_child {
                    Ok(parsed) => parsed.into_parts(),
                    Err(error)
                        if context.cursor() == element_start
                            && context.peek(0).map_or_else(
                                || error.span().start() as usize == context.source().len(),
                                |record| error.span() == record.span,
                            ) =>
                    {
                        return Err(context.unexpected(&#failure_value));
                    }
                    Err(error) => return Err(error),
                };
                let element_span = context.consumed_span_from(element_start);
                let #occurrence = context.finish_group(
                    #capture,
                    element_span,
                    [child_occurrence],
                )?;
                let #value = #wrapped;
            }
        }
        PlannedContainer::Optional {
            selector,
            child_follow,
        } => {
            let selector_name = expected_static(node_index, unique_index, "optional");
            let selector_definition =
                render_expected_static(&selector_name, selector, plan, facade);
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let absent = render_absent_value(wrappers);
            let direct_child = quote!(context.parse_node::<#terminal>(
                &#follow_value,
                #child_capture,
            ));
            let (attachment_definitions, parse_child) = attachment.map_or_else(
                || (quote!(), direct_child),
                |attachment| {
                    render_attachment_child(
                        plan,
                        node_index,
                        unique_index,
                        terminal,
                        attachment,
                        &follow_value,
                        &child_capture,
                        pratt_owner,
                        pratt_min_bp,
                    )
                },
            );
            quote! {
                #selector_definition
                #follow_definition
                let (#value, #occurrence) = if context.next_is(&#selector_name) {
                    let element_start = context.cursor();
                    #attachment_definitions
                    let (parsed_value, child_occurrence) = #parse_child?.into_parts();
                    let element_span = context.consumed_span_from(element_start);
                    let occurrence = context.finish_group(
                        #capture,
                        element_span,
                        [child_occurrence],
                    )?;
                    (#wrapped, occurrence)
                } else {
                    (#absent, context.finish_group(
                        #capture,
                        None,
                        ::std::iter::empty(),
                    )?)
                };
            }
        }
        PlannedContainer::Repeated {
            operation,
            element_selector,
            separator_selector,
            optional_trailing,
            stop,
            child_follow,
            minimum_failure,
            continuation_failure,
            after_separator_failure,
        } => render_repeated_parse(
            plan,
            node_index,
            unique_index,
            value,
            occurrence,
            terminal,
            wrappers,
            *operation,
            element_selector,
            separator_selector.as_ref(),
            *optional_trailing,
            stop,
            child_follow,
            minimum_failure.as_deref(),
            continuation_failure.as_ref(),
            after_separator_failure.as_deref(),
            capture,
            child_capture,
            attachment,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_attachment_child(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    terminal: &Type,
    attachment: &FixedAttachmentParsePlan,
    outer_follow: &proc_macro2::TokenStream,
    child_capture: &proc_macro2::TokenStream,
    pratt_owner: Option<&proc_macro2::TokenStream>,
    pratt_min_bp: Option<u8>,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let facade = &plan.facade;
    let PlannedContainer::Direct {
        child_follow,
        required_failure,
    } = attachment.semantic.as_ref()
    else {
        unreachable!("attachment semantic sites are frozen direct containers")
    };
    let semantic_follow_name =
        expected_static(node_index, unique_index, "attachment_semantic_follow");
    let (semantic_follow_definition, semantic_follow_value) =
        render_follow_value(plan, &semantic_follow_name, child_follow);
    let semantic_failure_name =
        expected_static(node_index, unique_index, "attachment_semantic_failure");
    let (semantic_failure_definition, semantic_failure_value) =
        render_follow_value(plan, &semantic_failure_name, required_failure);
    let semantic_parse = match (pratt_owner, pratt_min_bp) {
        (Some(owner), Some(min_bp)) => quote! {
            context.parse_generated(&#semantic_follow_value, #child_capture, |context| {
                #owner::__recursa_parse_bp(context, #min_bp)
            })
        },
        _ => quote! {
            context.parse_node::<#terminal>(&#semantic_follow_value, #child_capture)
        },
    };
    let before = attachment
        .before
        .iter()
        .enumerate()
        .map(|(index, fixed)| {
            let name = expected_static(
                node_index,
                unique_index,
                &format!("attachment_before_{index}"),
            );
            let definition = render_expected_static(&name, &fixed.expected, plan, facade);
            let occurrence = format_ident!("__recursa_attachment_before_{index}");
            let capture = fixed.capture.render(facade);
            (
                quote! {
                    #definition
                    let #occurrence = context.parse_fixed(&#name, #capture)?;
                },
                occurrence,
            )
        })
        .collect::<Vec<_>>();
    let after = attachment
        .after
        .iter()
        .enumerate()
        .map(|(index, fixed)| {
            let name = expected_static(
                node_index,
                unique_index,
                &format!("attachment_after_{index}"),
            );
            let definition = render_expected_static(&name, &fixed.expected, plan, facade);
            let occurrence = format_ident!("__recursa_attachment_after_{index}");
            let capture = fixed.capture.render(facade);
            (
                quote! {
                    #definition
                    let #occurrence = context.parse_fixed(&#name, #capture)?;
                },
                occurrence,
            )
        })
        .collect::<Vec<_>>();
    let before_statements = before.iter().map(|(statement, _)| statement);
    let before_occurrences = before.iter().map(|(_, occurrence)| occurrence);
    let after_statements = after.iter().map(|(statement, _)| statement);
    let after_occurrences = after.iter().map(|(_, occurrence)| occurrence);
    let parse = quote! {
        context.parse_generated(&#outer_follow, #child_capture, |context| {
            #semantic_follow_definition
            #semantic_failure_definition
            let attachment_start = context.cursor();
            #(#before_statements)*
            let semantic_start = context.cursor();
            let (parsed_value, semantic_occurrence) = match #semantic_parse {
                Ok(parsed) => parsed.into_parts(),
                Err(error)
                    if context.cursor() == semantic_start
                        && context.peek(0).map_or_else(
                            || error.span().start() as usize == context.source().len(),
                            |record| error.span() == record.span,
                        ) =>
                {
                    return Err(context.unexpected(&#semantic_failure_value));
                }
                Err(error) => return Err(error),
            };
            #(#after_statements)*
            let attachment_span = context.consumed_span_from(attachment_start);
            context.finish_with_children(
                parsed_value,
                attachment_span,
                [
                    #(#before_occurrences,)*
                    semantic_occurrence,
                    #(#after_occurrences,)*
                ],
            )
        })
    };
    (quote!(), parse)
}

#[allow(clippy::too_many_arguments)]
fn render_repeated_parse(
    plan: &EmissionPlan,
    node_index: usize,
    unique_index: usize,
    value: &Ident,
    occurrence: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    operation: ContainerOperation,
    element_selector: &ExpectedSet,
    separator_selector: Option<&ExpectedSet>,
    optional_trailing: bool,
    stop: &PlannedFollow,
    child_follow: &PlannedFollow,
    minimum_failure: Option<&PlannedFollow>,
    continuation_failure: &PlannedFollow,
    after_separator_failure: Option<&PlannedFollow>,
    capture: proc_macro2::TokenStream,
    child_capture: proc_macro2::TokenStream,
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let repeat_index = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore))
        .expect("a repeated container plan has a repetition wrapper");
    let outer = &wrappers[..repeat_index];
    let inner = &wrappers[repeat_index + 1..];
    let repeated_wrapper = wrappers[repeat_index];
    let wrapped_item = render_wrapped_value(quote!(parsed_value), inner);

    let element_name = expected_static(node_index, unique_index, "element");
    let element_definition = render_expected_static(&element_name, element_selector, plan, facade);
    let stop_name = expected_static(node_index, unique_index, "stop");
    let (stop_definition, stop_value) = render_follow_value(plan, &stop_name, stop);
    let child_follow_name = expected_static(node_index, unique_index, "child_follow");
    let (child_follow_definition, child_follow_value) =
        render_follow_value(plan, &child_follow_name, child_follow);
    let minimum_name = expected_static(node_index, unique_index, "minimum_failure");
    let (minimum_definition, minimum_value) = minimum_failure.map_or_else(
        || (quote!(), quote!()),
        |failure| render_follow_value(plan, &minimum_name, failure),
    );
    let continuation_name = expected_static(node_index, unique_index, "continuation_failure");
    let (continuation_definition, continuation_value) =
        render_follow_value(plan, &continuation_name, continuation_failure);
    let after_separator_name = expected_static(node_index, unique_index, "after_separator_failure");
    let (after_separator_definition, after_separator_value) = after_separator_failure.map_or_else(
        || (quote!(), quote!()),
        |failure| render_follow_value(plan, &after_separator_name, failure),
    );
    let direct_child = quote!(context.parse_node::<#terminal>(
        &#child_follow_value,
        #child_capture,
    ));
    let (attachment_definitions, parse_child) = attachment.map_or_else(
        || (quote!(), direct_child),
        |attachment| {
            render_attachment_child(
                plan,
                node_index,
                unique_index,
                terminal,
                attachment,
                &child_follow_value,
                &child_capture,
                None,
                None,
            )
        },
    );
    let parse_item = quote! {
        let (parsed_value, child_occurrence) = #parse_child?.into_parts();
        parsed_values.push(#wrapped_item);
        child_occurrences.push(child_occurrence);
    };
    let start = if operation == ContainerOperation::OneOrMore {
        quote! {
            if !context.next_is(&#element_name) {
                return Err(context.unexpected(&#minimum_value));
            }
            #parse_item
        }
    } else if minimum_failure.is_some() {
        quote! {
            if context.next_is(&#element_name) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#minimum_value));
            }
        }
    } else {
        quote! {
            if context.next_is(&#element_name) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#continuation_value));
            }
        }
    };

    let continuation = if let Some(separator_selector) = separator_selector {
        let separator_name = expected_static(node_index, unique_index, "separator");
        let separator_definition =
            render_expected_static(&separator_name, separator_selector, plan, facade);
        let trailing = if optional_trailing {
            quote! {
                if context.is_eof() || context.next_is(&#stop_value) {
                    break;
                }
            }
        } else {
            quote! {}
        };
        quote! {
            #separator_definition
            loop {
                if !context.next_is(&#separator_name) {
                    if context.is_eof() || context.next_is(&#stop_value) {
                        break;
                    }
                    return Err(context.unexpected(&#continuation_value));
                }
                let _separator = context.parse_fixed(
                    &#separator_name,
                    #facade::__private::CapturePolicy::Off,
                )?;
                if context.next_is(&#element_name) {
                    #parse_item
                    continue;
                }
                #trailing
                return Err(context.unexpected(&#after_separator_value));
            }
        }
    } else {
        quote! {
            loop {
                if context.next_is(&#element_name) {
                    #parse_item
                    continue;
                }
                if context.is_eof() || context.next_is(&#stop_value) {
                    break;
                }
                return Err(context.unexpected(&#continuation_value));
            }
        }
    };

    let collection = match repeated_wrapper {
        FieldWrapper::ZeroOrMore => quote!(parsed_values),
        FieldWrapper::OneOrMore => quote!({
            let mut values = parsed_values.into_iter();
            let first = values
                .next()
                .expect("a present one-or-more container parsed its first value");
            let mut non_empty = #facade::Vec1::new(first);
            for value in values {
                non_empty.push(value);
            }
            non_empty
        }),
        FieldWrapper::Boxed | FieldWrapper::Optional => unreachable!(),
    };
    let present = render_wrapped_value(collection, outer);
    let has_values = if operation == ContainerOperation::OptionalOneOrMore {
        quote!(let has_values = !parsed_values.is_empty();)
    } else {
        quote!()
    };
    let semantic = if operation == ContainerOperation::OptionalOneOrMore {
        let absent = render_absent_value(outer);
        quote! {
            let #value = if !has_values {
                #absent
            } else {
                #present
            };
        }
    } else {
        quote!(let #value = #present;)
    };

    quote! {
        #element_definition
        #stop_definition
        #child_follow_definition
        #minimum_definition
        #continuation_definition
        #after_separator_definition
        #attachment_definitions
        let element_start = context.cursor();
        let mut parsed_values = Vec::new();
        let mut child_occurrences = #facade::__private::Occurrences::new();
        #start
        if !parsed_values.is_empty() {
            #continuation
        }
        let element_span = context.consumed_span_from(element_start);
        #has_values
        let #occurrence = context.finish_group(
            #capture,
            element_span,
            child_occurrences,
        )?;
        #semantic
    }
}

fn render_follow_value(
    plan: &EmissionPlan,
    name: &Ident,
    follow: &PlannedFollow,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let facade = &plan.facade;
    let value = format_ident!("{}_value", name.to_string().to_ascii_lowercase());
    match follow {
        PlannedFollow::Caller => (
            quote!(let #value = context.follow().clone();),
            quote!(#value),
        ),
        PlannedFollow::Static(expected) => {
            let definition = render_expected_static(name, expected, plan, facade);
            (
                quote!(#definition let #value = #name.clone();),
                quote!(#value),
            )
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let definition = render_expected_static(name, expected, plan, facade);
            (
                quote! {
                    #definition
                    let #value = #name
                        .union(context.follow())
                        .expect("matching generated FOLLOW widths");
                },
                quote!(#value),
            )
        }
    }
}

fn render_construction(
    variant: Option<&Ident>,
    style: ResolvedFieldStyle,
    values: &[(FieldPlan, Ident)],
) -> proc_macro2::TokenStream {
    let target = variant.map_or_else(|| quote!(Self), |variant| quote!(Self::#variant));
    match style {
        ResolvedFieldStyle::Named => {
            let fields = values.iter().map(|(field, value)| {
                let ident = field
                    .ident
                    .as_ref()
                    .expect("named source shape has named fields");
                quote!(#ident: #value)
            });
            quote!(#target { #(#fields),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let mut ordered = values.iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(field, _)| field.index);
            let fields = ordered.into_iter().map(|(_, value)| value);
            quote!(#target(#(#fields),*))
        }
        ResolvedFieldStyle::Unit => quote!(#target),
    }
}

fn render_wrapped_value(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
) -> proc_macro2::TokenStream {
    wrappers
        .iter()
        .rev()
        .fold(value, |value, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!(Box::new(#value)),
            FieldWrapper::Optional => quote!(Some(#value)),
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("a repetition wrapper is constructed separately")
            }
        })
}

fn render_absent_value(wrappers: &[FieldWrapper]) -> proc_macro2::TokenStream {
    let optional = wrappers
        .iter()
        .position(|wrapper| matches!(wrapper, FieldWrapper::Optional))
        .expect("an absent field has an optional wrapper");
    wrappers[..optional]
        .iter()
        .rev()
        .fold(quote!(None), |value, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!(Box::new(#value)),
            FieldWrapper::Optional => unreachable!("the first optional ends the absent value"),
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("an absent outer wrapper cannot contain a repetition")
            }
        })
}

fn render_wrapped_type(facade: &Ident, terminal: &Type, wrappers: &[FieldWrapper]) -> Type {
    wrappers
        .iter()
        .rev()
        .fold(terminal.clone(), |ty, wrapper| match wrapper {
            FieldWrapper::Boxed => syn::parse_quote!(Box<#ty>),
            FieldWrapper::Optional => syn::parse_quote!(Option<#ty>),
            FieldWrapper::ZeroOrMore => syn::parse_quote!(Vec<#ty>),
            FieldWrapper::OneOrMore => syn::parse_quote!(#facade::Vec1<#ty>),
        })
}

fn render_terminal_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let initial = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    wrappers
        .iter()
        .fold(initial, |reference, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!((#reference).as_ref()),
            FieldWrapper::Optional => {
                unreachable!("non-optional accessors contain no optional wrapper")
            }
            FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore => {
                unreachable!("container access stops at the repetition wrapper")
            }
        })
}

fn render_optional_view_reference(
    value: proc_macro2::TokenStream,
    wrappers: &[FieldWrapper],
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    let mut reference = if value_is_reference {
        value
    } else {
        quote!(&#value)
    };
    let mut optional = false;
    for wrapper in wrappers {
        match (optional, wrapper) {
            (false, FieldWrapper::Boxed) => reference = quote!((#reference).as_ref()),
            (false, FieldWrapper::Optional) => {
                reference = quote!((#reference).as_ref());
                optional = true;
            }
            (true, FieldWrapper::Boxed) => {
                reference = quote!((#reference).map(|value| value.as_ref()));
            }
            (true, FieldWrapper::Optional) => {
                reference = quote!((#reference).and_then(|value| value.as_ref()));
            }
            (_, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore) => {
                unreachable!("container access stops at the repetition wrapper")
            }
        }
    }
    debug_assert!(optional);
    reference
}

fn field_access(
    base: proc_macro2::TokenStream,
    ident: Option<&Ident>,
    index: usize,
) -> proc_macro2::TokenStream {
    ident.map_or_else(
        || {
            let index = syn::Index::from(index);
            quote!(#base.#index)
        },
        |ident| quote!(#base.#ident),
    )
}

fn field_binding_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
    field: Option<&Ident>,
    index: usize,
    binding: &Ident,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let field = field.expect("named variant field has an identifier");
            quote!(#node::#variant { #field: #binding, .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let before = (0..index).map(|_| quote!(_));
            quote!(#node::#variant(#(#before,)* #binding, ..))
        }
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

fn variant_pattern(
    node: &Ident,
    variant: &Ident,
    style: ResolvedFieldStyle,
) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => quote!(#node::#variant { .. }),
        ResolvedFieldStyle::Unnamed => quote!(#node::#variant(..)),
        ResolvedFieldStyle::Unit => quote!(#node::#variant),
    }
}

fn render_expected_static(
    name: &Ident,
    expected: &ExpectedSet,
    plan: &EmissionPlan,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let words = &expected.words;
    let atoms = expected
        .atoms
        .iter()
        .map(|atom| quote!(#facade::__private::ExpectedAtomId(#atom)));
    let kind_count = plan.kind_count;
    quote! {
        static #name: #facade::__private::FollowSet = #facade::__private::follow_set(
            #kind_count as u32,
            &[#(#words),*],
            &[#(#atoms),*],
        );
    }
}

fn metadata_static(node: usize, variant: Option<usize>) -> Ident {
    variant.map_or_else(
        || format_ident!("__RECURSA_ELEMENTS_{node}"),
        |variant| format_ident!("__RECURSA_ELEMENTS_{node}_{variant}"),
    )
}

fn expected_static(node: usize, element: usize, role: &str) -> Ident {
    format_ident!("__RECURSA_{}_{}_{}", role.to_uppercase(), node, element)
}

fn generic_arguments(generics: &Generics) -> Vec<proc_macro2::TokenStream> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => {
                let lifetime = &parameter.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
            GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
        })
        .collect()
}

fn publish(out_dir: &Path, identity: &str, rendered: &RenderedGeneration) -> Result<(), String> {
    publish_with_checkpoint(out_dir, identity, rendered, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicationCheckpoint {
    BeforeStaging,
    DuringStaging,
    BeforeSelection,
    DuringSelection,
}

fn publish_with_checkpoint(
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

fn stage_and_publish(
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

fn select_generation(
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

fn invalidate_selection(out_dir: &Path) -> Result<(), String> {
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
fn selected_generation(out_dir: &Path) -> Option<String> {
    let selector = fs::read_to_string(out_dir.join(SELECTED_GENERATION)).ok()?;
    selector
        .lines()
        .next()?
        .strip_prefix("// recursa-generation:")
        .map(str::to_owned)
}

fn staging_nonce() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock cannot name staging output: {error}"))
        .map(|duration| duration.as_nanos())
}

fn emitted_ident(name: &str) -> Result<Ident, String> {
    syn::parse_str::<Ident>(name)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{name}")))
        .map_err(|error| format!("cannot emit Rust identifier `{name}`: {error}"))
}

fn emitted_crate_path(segments: &[String]) -> Result<syn::Path, String> {
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    Ok(path)
}

fn sibling_ident(ident: &Ident, suffix: &str) -> Ident {
    Ident::new(
        &format!("{}{suffix}", ident.unraw()),
        proc_macro2::Span::call_site(),
    )
}

fn snake_case(value: &str) -> String {
    if value
        .chars()
        .any(|character| character.is_ascii_alphabetic())
        && value
            .chars()
            .all(|character| !character.is_ascii_alphabetic() || character.is_ascii_uppercase())
    {
        let lowered = value.to_ascii_lowercase();
        let trimmed = lowered.trim_matches('_');
        return if trimmed.is_empty() {
            "element".to_owned()
        } else {
            trimmed.to_owned()
        };
    }
    let mut output = String::new();
    let mut previous_was_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase()
                && !output.is_empty()
                && !previous_was_separator
                && output
                    .chars()
                    .last()
                    .is_some_and(|last| last.is_ascii_lowercase())
            {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !output.is_empty() && !previous_was_separator {
            output.push('_');
            previous_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        "element".to_owned()
    } else {
        output
    }
}

fn source_line_column(source: &str, byte_offset: u32) -> Result<(usize, usize), String> {
    let prefix = source
        .get(..byte_offset as usize)
        .ok_or_else(|| "the Node item position is not a UTF-8 source boundary".to_owned())?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    Ok((line, column))
}

fn normalized_source_identity(path: &Path) -> Result<String, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => segments.push(
                segment
                    .to_str()
                    .ok_or_else(|| "the Node source path is not valid UTF-8".to_owned())?,
            ),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("the Node source identity is not manifest-relative".to_owned());
            }
        }
    }
    if segments.is_empty() {
        return Err("the Node source identity is empty".to_owned());
    }
    Ok(segments.join("/"))
}

fn declaration_loader_anchor(file: &str, line: usize, column: usize, name: &str) -> String {
    let input = format!("{file}:{line}:{column}:{name}");
    format!("{:016x}", loader_anchor_hash(input.as_bytes()))
}

fn loader_anchor_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf_29ce_4842_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_visit_variant_uses_the_path_relative_to_the_grammar_module() {
        assert_eq!(
            visit_variant_name("grammar", "grammar::public_left::Duplicate"),
            "__RecursaTarget_11_public_left_9_Duplicate"
        );
    }

    #[test]
    fn root_visit_variants_are_encoded_away_from_reserved_and_nested_names() {
        assert_eq!(
            visit_variant_name("grammar", "grammar::Opaque"),
            "__RecursaTarget_6_Opaque"
        );
        assert_ne!(
            visit_variant_name(
                "grammar",
                "grammar::__RecursaTarget_11_public_left_9_Duplicate"
            ),
            visit_variant_name("grammar", "grammar::public_left::Duplicate")
        );
    }

    #[test]
    fn snake_case_produces_stable_generated_element_names() {
        assert_eq!(snake_case("RECORD"), "record");
        assert_eq!(snake_case("LabelColon"), "label_colon");
        assert_eq!(snake_case(":"), "element");
    }

    #[test]
    fn synthetic_fixed_spelling_uppercases_only_ascii_insensitive_keywords() {
        assert_eq!(
            fixed_pretty_spelling("record", TextEquivalence::AsciiInsensitive),
            "RECORD"
        );
        assert_eq!(
            fixed_pretty_spelling(":", TextEquivalence::AsciiInsensitive),
            ":"
        );
        assert_eq!(
            fixed_pretty_spelling("record", TextEquivalence::Sensitive),
            "record"
        );
    }

    #[test]
    fn fixed_accessor_names_remain_valid_for_numeric_spellings() {
        assert_eq!(
            super::fixed_accessor_ident("123").unwrap().to_string(),
            "element_123_span"
        );
    }

    #[test]
    fn emission_failures_use_the_registered_m4_code_and_preserve_detail() {
        let error = failure(
            Path::new("src/grammar.rs"),
            "the frozen emission plan is incomplete",
        );
        let [diagnostic] = error.report().diagnostics() else {
            panic!("one emission invariant failure must produce one diagnostic");
        };
        assert_eq!(diagnostic.code().as_str(), "RCA9105");
        assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
        assert!(matches!(
            diagnostic.evidence(),
            crate::DiagnosticEvidence::InternalInvariant { producer, detail }
                if producer == "Emission" && detail == "the frozen emission plan is incomplete"
        ));
    }

    #[test]
    fn predictive_compilation_rejects_missing_and_conflicting_frozen_products() {
        let position = DispatchPositionPlan {
            expected: ExpectedSet {
                words: vec![1],
                atoms: vec![0],
            },
            text_predicate: None,
        };
        let root = DispatchTrieNodePlan {
            target: None,
            edges: vec![DispatchTrieEdgePlan {
                position,
                next: DispatchTrieNodePlan {
                    target: Some(DispatchTargetPlan {
                        variant: 0,
                        on_caller_follow: false,
                    }),
                    edges: Vec::new(),
                },
            }],
        };
        let mut dispatch = PredictiveDispatchPlan {
            frozen_identity: predictive_dispatch_identity(&root),
            root,
            frozen_sequence_count: 1,
            frozen_node_count: 2,
        };
        assert_eq!(validate_predictive_dispatch(&dispatch, 1).unwrap(), 2);
        dispatch.root.edges[0].next.target.as_mut().unwrap().variant = 1;
        assert_eq!(
            validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
            "frozen predictive dispatch references an unknown variant",
        );

        dispatch.root.edges[0].next.target.as_mut().unwrap().variant = 0;
        dispatch.root.edges[0].position.text_predicate =
            Some(PlannedTextPredicate::Exact("corrupt".to_owned()));
        assert_eq!(
            validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
            "the joined predictive plan diverges from its frozen identity",
        );
    }

    #[test]
    fn corrupted_joined_dispatch_fails_before_publication_with_rca9105() {
        let package = tempfile::tempdir().expect("temporary predictive plan fixture");
        fs::write(
            package.path().join("grammar.rs"),
            r#"
recursa::grammar! { module = crate, max_lookahead = 1 }

#[derive(recursa::Node)]
enum Root { A(A), B(B) }

#[derive(recursa::Node)]
#[tok("a")]
struct A;

#[derive(recursa::Node)]
#[tok("b")]
struct B;
"#,
        )
        .expect("write predictive plan fixture");
        let cfg = CargoCfgSnapshot::default();
        let success = analyze_source_tree(
            M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("analyze predictive plan fixture");
        let mut plan = EmissionPlan::join(&success.analyzed, &cfg)
            .expect("join a complete predictive Emission plan");
        assert!(plan.instrumentation.frozen_dispatch_nodes_consumed > 0);
        render(&plan).expect("the renderer consumes every frozen trie node");

        let dispatch = plan
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.body {
                NodeBodyPlan::Enum { dispatch, .. } => Some(dispatch),
                NodeBodyPlan::Struct { .. } | NodeBodyPlan::Pratt(_) => None,
            })
            .expect("fixture contains one predictive dispatch");
        dispatch.root.edges[0].position.text_predicate =
            Some(PlannedTextPredicate::Exact("corrupt".to_owned()));

        let detail = match render(&plan) {
            Err(detail) => detail,
            Ok(_) => panic!("corrupt joined plans cannot reach publication"),
        };
        let error = failure(Path::new("grammar.rs"), &detail);
        let [diagnostic] = error.report().diagnostics() else {
            panic!("one pre-publication invariant diagnostic is required");
        };
        assert_eq!(diagnostic.code().as_str(), "RCA9105");
        assert!(!package.path().join(SELECTED_GENERATION).exists());
    }

    fn joined_pratt_fixture() -> (tempfile::TempDir, EmissionPlan) {
        joined_pratt_fixture_from(include_str!("../tests/fixtures/m4/pratt/src/grammar.rs"))
    }

    #[cfg(feature = "diagrams")]
    fn joined_pratt_diagram_fixture() -> (tempfile::TempDir, EmissionPlan) {
        let source = include_str!("../tests/fixtures/m4/pratt/src/grammar.rs").replacen(
            "module = crate::grammar,",
            "module = crate::grammar,\n    diagrams,",
            1,
        );
        joined_pratt_fixture_from(&source)
    }

    fn joined_pratt_fixture_from(source: &str) -> (tempfile::TempDir, EmissionPlan) {
        let package = tempfile::tempdir().expect("temporary Pratt plan fixture");
        fs::write(package.path().join("grammar.rs"), source).expect("write Pratt plan fixture");
        let success = analyze_source_tree(
            M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("analyze Pratt plan fixture");
        let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
            .expect("join a complete Pratt Emission plan");
        render(&plan).expect("a complete Pratt plan renders");
        (package, plan)
    }

    fn assert_corrupt_pratt_plan_is_unselectable(package: &Path, plan: &EmissionPlan) {
        let detail = match render(plan) {
            Err(detail) => detail,
            Ok(_) => panic!("corrupt Pratt plans cannot reach publication"),
        };
        let error = failure(Path::new("grammar.rs"), &detail);
        let [diagnostic] = error.report().diagnostics() else {
            panic!("one pre-publication invariant diagnostic is required");
        };
        assert_eq!(diagnostic.code().as_str(), "RCA9105");
        assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
        assert!(!package.join(SELECTED_GENERATION).exists());
    }

    #[test]
    fn corrupted_pratt_branch_follow_and_failure_identities_fail_before_publication() {
        let (package, mut missing_plan) = joined_pratt_fixture();
        let pratt = missing_plan
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.body {
                NodeBodyPlan::Pratt(pratt) => Some(pratt),
                NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
            })
            .expect("fixture contains a Pratt plan");
        pratt.nud.clear();
        assert_corrupt_pratt_plan_is_unselectable(package.path(), &missing_plan);

        let (package, mut branch_plan) = joined_pratt_fixture();
        let pratt = branch_plan
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.body {
                NodeBodyPlan::Pratt(pratt) => Some(pratt),
                NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
            })
            .expect("fixture contains a Pratt plan");
        let variant_count = pratt.nud.len();
        let mut pending = vec![&mut pratt.nud_dispatch.root];
        let target = loop {
            let node = pending.pop().expect("nud dispatch contains a terminal");
            if let Some(target) = &mut node.target {
                break target;
            }
            pending.extend(node.edges.iter_mut().map(|edge| &mut edge.next));
        };
        target.variant = (target.variant + 1) % variant_count;
        assert_corrupt_pratt_plan_is_unselectable(package.path(), &branch_plan);

        let (package, mut follow_plan) = joined_pratt_fixture();
        let pratt = follow_plan
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.body {
                NodeBodyPlan::Pratt(pratt) => Some(pratt),
                NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
            })
            .expect("fixture contains a Pratt plan");
        let follow = pratt
            .led
            .iter_mut()
            .flat_map(|branch| &mut branch.variant.elements)
            .find_map(|element| match &mut element.kind {
                ElementParseKind::Semantic {
                    container,
                    pratt_min_bp: Some(_),
                    ..
                } => match container.as_mut() {
                    PlannedContainer::Direct { child_follow, .. } => Some(child_follow),
                    PlannedContainer::Optional { .. } | PlannedContainer::Repeated { .. } => None,
                },
                ElementParseKind::Fixed { .. } | ElementParseKind::Semantic { .. } => None,
            })
            .expect("fixture contains a recursive Pratt operand FOLLOW");
        *follow = PlannedFollow::Caller;
        assert_corrupt_pratt_plan_is_unselectable(package.path(), &follow_plan);

        let (package, mut failure_plan) = joined_pratt_fixture();
        let pratt = failure_plan
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.body {
                NodeBodyPlan::Pratt(pratt) => Some(pratt),
                NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
            })
            .expect("fixture contains a Pratt plan");
        pratt.nud_failure = PlannedFollow::Caller;
        assert_corrupt_pratt_plan_is_unselectable(package.path(), &failure_plan);
    }

    #[test]
    fn corrupted_real_plan_family_content_fails_before_publication() {
        let (package, mut traits) = joined_pratt_fixture();
        traits.nodes[0]
            .configured_traits
            .pop()
            .expect("the trait fixture configures recipes");
        assert_plan_family_is_unselectable(package.path(), &traits, "configured-trait");

        let (package, mut private_abi) = joined_pratt_fixture();
        private_abi.private_abi.protocol += 1;
        assert_plan_family_is_unselectable(package.path(), &private_abi, "private ABI");

        let (package, mut capabilities) = joined_pratt_fixture();
        capabilities
            .capabilities
            .pop()
            .expect("the fixture accepts at least one exact facade capability");
        assert_plan_family_is_unselectable(package.path(), &capabilities, "capability");

        let (package, mut pretty) = joined_pratt_fixture();
        let element = pretty
            .pretty
            .nodes
            .iter_mut()
            .flatten()
            .flat_map(|node| &mut node.variants)
            .flat_map(|variant| variant.iter_mut())
            .next()
            .expect("the trait fixture contains Pretty behavior");
        match element {
            PrettyElementBehavior::Fixed { spelling, .. } => spelling.push_str("-corrupt"),
            PrettyElementBehavior::Semantic { repeated, .. } => *repeated = !*repeated,
        }
        assert_plan_family_is_unselectable(package.path(), &pretty, "Pretty");

        let (package, mut attachment) = joined_pratt_fixture();
        let fixed = attachment
            .pretty
            .nodes
            .iter_mut()
            .flatten()
            .flat_map(|node| &mut node.variants)
            .flat_map(|variant| variant.iter_mut())
            .find_map(|element| match element {
                PrettyElementBehavior::Semantic {
                    attachment: Some(attachment),
                    ..
                } => attachment.before.first_mut(),
                PrettyElementBehavior::Fixed { .. }
                | PrettyElementBehavior::Semantic {
                    attachment: None, ..
                } => None,
            })
            .expect("the Pratt fixture contains a fixed attachment");
        fixed.spelling.push_str("-corrupt");
        assert_plan_family_is_unselectable(package.path(), &attachment, "Pretty");

        #[cfg(feature = "diagrams")]
        {
            let (package, mut diagram) = joined_pratt_diagram_fixture();
            diagram
                .diagrams
                .as_mut()
                .expect("diagram request creates emitted content")
                .nodes[0]
                .production
                .push_str(" corrupt");
            assert_plan_family_is_unselectable(package.path(), &diagram, "diagram");
        }
        #[cfg(not(feature = "diagrams"))]
        {
            let (_, plan) = joined_pratt_fixture();
            assert_eq!(plan.plan_families.diagrams, None);
        }

        let (package, mut explorer) = joined_pratt_fixture();
        explorer.explorer = Some(ExtensionEmissionPlan {
            artifacts: vec![ExtensionArtifactPlan {
                logical_name: "grammar-index".to_owned(),
                content: "corrupt explorer plan".to_owned(),
            }],
        });
        assert_plan_family_is_unselectable(package.path(), &explorer, "explorer");
    }

    fn assert_plan_family_is_unselectable(package: &Path, plan: &EmissionPlan, family: &str) {
        let detail = match render(plan) {
            Err(detail) => detail,
            Ok(_) => panic!("a corrupted family cannot render"),
        };
        assert!(detail.contains(family), "detail={detail}");
        let error = failure(Path::new("grammar.rs"), &detail);
        let [diagnostic] = error.report().diagnostics() else {
            panic!("one pre-publication invariant diagnostic is required");
        };
        assert_eq!(diagnostic.code().as_str(), "RCA9105");
        assert!(!package.join(SELECTED_GENERATION).exists());
    }

    #[test]
    fn incomplete_staging_is_unselectable() {
        let output = tempfile::tempdir().unwrap();
        let staging = output.path().join("staging");
        let published = output.path().join("published");
        fs::create_dir(&staging).unwrap();
        let rendered = RenderedGeneration {
            root: "fn root() {}".to_owned(),
            nodes: vec![("node-0.rs".to_owned(), "fn node() {}".to_owned())],
            node_anchors: vec!["anchor-0".to_owned()],
            selector: "fn selected() {}".to_owned(),
        };
        assert_eq!(
            stage_and_publish(&staging, &published, &rendered, || {
                Err("injected staging interruption".to_owned())
            })
            .unwrap_err(),
            "injected staging interruption",
        );
        assert!(!published.exists());
        assert!(staging.join("root.rs").is_file());
        assert!(!staging.join("node-0.rs").exists());
    }

    fn publication_fixture(identity: &str) -> RenderedGeneration {
        RenderedGeneration {
            root: format!("// {identity} root"),
            nodes: vec![("node-0.rs".to_owned(), format!("// {identity} node"))],
            node_anchors: vec!["anchor-0".to_owned()],
            selector: format!(
                "// recursa-generation:{identity}\ninclude!(concat!(env!(\"OUT_DIR\"), \"/recursa-generations/{identity}/root.rs\"));\n"
            ),
        }
    }

    #[test]
    fn every_failed_publication_checkpoint_invalidates_stale_selection_and_recovers() {
        for checkpoint in [
            PublicationCheckpoint::BeforeStaging,
            PublicationCheckpoint::DuringStaging,
            PublicationCheckpoint::BeforeSelection,
            PublicationCheckpoint::DuringSelection,
        ] {
            let output = tempfile::tempdir().unwrap();
            let old = publication_fixture("g_old");
            publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
            assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

            invalidate_selection(output.path()).unwrap();
            let next = publication_fixture("g_next");
            let error = publish_with_checkpoint(output.path(), "g_next", &next, |point| {
                if point == checkpoint {
                    Err(format!("injected {point:?} fault"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(error, format!("injected {checkpoint:?} fault"));
            assert_eq!(selected_generation(output.path()), None);

            publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
            assert_eq!(
                selected_generation(output.path()).as_deref(),
                Some("g_next")
            );
            assert!(
                output
                    .path()
                    .join("recursa-generations/g_next/root.rs")
                    .is_file()
            );
            assert!(
                output
                    .path()
                    .join("recursa-generations/g_next/node-0.rs")
                    .is_file()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_invalidation_failure_poisons_stale_selection_and_recovers() {
        use std::os::unix::fs::PermissionsExt as _;

        let output = tempfile::tempdir().unwrap();
        let old = publication_fixture("g_old");
        publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
        assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

        let original_permissions = fs::metadata(output.path()).unwrap().permissions();
        let mut non_writable = original_permissions.clone();
        non_writable.set_mode(0o555);
        fs::set_permissions(output.path(), non_writable).unwrap();
        let error = invalidate_selection(output.path()).unwrap_err();
        fs::set_permissions(output.path(), original_permissions).unwrap();

        assert!(error.contains("cannot invalidate"), "{error}");
        assert_eq!(selected_generation(output.path()), None);
        let poisoned = fs::read_to_string(output.path().join(SELECTED_GENERATION)).unwrap();
        assert!(poisoned.contains("compile_error!"));
        assert!(!poisoned.contains("recursa-generation:g_old"));

        invalidate_selection(output.path()).unwrap();
        let next = publication_fixture("g_next");
        publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
        assert_eq!(
            selected_generation(output.path()).as_deref(),
            Some("g_next")
        );
    }

    #[test]
    fn filesystem_rename_failures_leave_no_loadable_or_staged_generation() {
        let output = tempfile::tempdir().unwrap();
        let rendered = publication_fixture("g_next");
        let published = output.path().join("recursa-generations/g_next");
        let error = publish_with_checkpoint(output.path(), "g_next", &rendered, |checkpoint| {
            if checkpoint == PublicationCheckpoint::DuringStaging {
                fs::create_dir_all(&published).unwrap();
                fs::write(published.join("block"), "external collision").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("publication barrier"), "{error}");
        assert_eq!(selected_generation(output.path()), None);
        assert_no_transaction_staging(output.path());

        fs::remove_dir_all(&published).unwrap();
        fs::create_dir(output.path().join(SELECTED_GENERATION)).unwrap();
        let error =
            publish_with_checkpoint(output.path(), "g_next", &rendered, |_| Ok(())).unwrap_err();
        assert!(error.contains("atomically select"), "{error}");
        assert_eq!(selected_generation(output.path()), None);
        assert_no_transaction_staging(output.path());
    }

    #[test]
    fn warnings_publish_one_complete_generation_and_remain_on_the_result() {
        let package = tempfile::tempdir().unwrap();
        fs::write(
            package.path().join("grammar.rs"),
            r#"
recursa::grammar! { module = crate, max_lookahead = 1 }
recursa::tokens! { keywords { SELECT => "SELECT" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Select;

#[derive(recursa::Node)]
struct Inner { maybe: Option<Select> }

#[derive(recursa::Node)]
struct Root {
    #[sep(SELECT)]
    items: Vec<Inner>,
}
"#,
        )
        .unwrap();
        let success = analyze_source_tree(
            M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("warnings retain analyzable output");
        assert!(success.build_report.warnings().next().is_some());
        let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default()).unwrap();
        let rendered = render(&plan).unwrap();
        let output = tempfile::tempdir().unwrap();
        publish(output.path(), &plan.identity, &rendered).unwrap();

        let generation = Generation {
            report: success.build_report,
        };
        assert!(generation.report().warnings().next().is_some());
        assert_eq!(
            selected_generation(output.path()).as_deref(),
            Some(plan.identity.as_str())
        );
        let published = output
            .path()
            .join("recursa-generations")
            .join(&plan.identity);
        assert!(published.join("root.rs").is_file());
        assert!(published.join("loader-map").is_file());
        assert_eq!(
            fs::read_dir(published)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("node-"))
                .count(),
            rendered.nodes.len()
        );
    }

    fn assert_no_transaction_staging(out_dir: &Path) {
        let mut pending = vec![out_dir.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let name = path.file_name().unwrap().to_string_lossy();
                    assert!(!name.starts_with(".recursa-selector-"), "{path:?}");
                }
            }
        }
        let generations = out_dir.join("recursa-generations");
        if let Ok(entries) = fs::read_dir(generations) {
            for entry in entries {
                let path = entry.unwrap().path();
                assert!(
                    !path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".staging-"),
                    "{path:?}"
                );
            }
        }
    }
}
