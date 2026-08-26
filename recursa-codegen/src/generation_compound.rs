//! M4 source-aware generation for authored Node shapes.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use quote::{ToTokens as _, format_ident, quote};
use syn::ext::IdentExt as _;
use syn::spanned::Spanned as _;
use syn::{GenericArgument, GenericParam, Generics, Ident, PathArguments, Type, Visibility};

use crate::TypeShape;
use crate::build_diagnostic::{BuildReport, DiagnosticStage, GenerateError};
use crate::decision_plan::{ContainerOperation, DispatchKind, FollowRecipe};
use crate::lexical::{LexicalMatcher, TokenKindIdentity};
use crate::lowering::ModuleId;
use crate::m3::{CargoOutput, M3Input, analyze_source_tree, internal_generate_error};
use crate::materialization::{
    ClosedType, MaterializedElement, MaterializedNodeApplication, MaterializedNodeBody,
    MaterializedVariant,
};
use crate::resolution::{ResolvedFieldStyle, ResolvedNode, ResolvedNodeBody};
use crate::source_model::TokenDefinitionKind;
use crate::source_syntax::{CaptureMode, CargoCfgSnapshot, span_range};
use crate::{FirstPosition, KindSet, SiteId, TextEquivalence};

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
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "CARGO_MANIFEST_DIR is unavailable"))?;
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "OUT_DIR is unavailable"))?;
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
    kind_count: usize,
    lexical_error_kind: u16,
    kind_variants: Vec<Ident>,
    kind_names: Vec<String>,
    atom_names: Vec<String>,
    nodes: Vec<NodeParsePlan>,
    tokens: Vec<TokenParsePlan>,
    token_bindings: Vec<TokenBindingPlan>,
    lexical_rules: Vec<LexRulePlan>,
}

#[derive(Clone)]
struct NodeParsePlan {
    semantic_key: String,
    module: ModuleId,
    ident: Ident,
    visibility: Visibility,
    loader_anchor: String,
    generics: Generics,
    input_lifetime: syn::Lifetime,
    body: NodeBodyPlan,
    capture: PlannedCapturePolicy,
}

#[derive(Clone)]
enum NodeBodyPlan {
    Struct {
        style: ResolvedFieldStyle,
        elements: Vec<ElementParsePlan>,
    },
    Enum {
        variants: Vec<VariantParsePlan>,
        expected: ExpectedSet,
    },
}

#[derive(Clone)]
struct VariantParsePlan {
    ident: Ident,
    style: ResolvedFieldStyle,
    elements: Vec<ElementParsePlan>,
    selector: ExpectedSet,
    capture: Option<PlannedCapturePolicy>,
}

#[derive(Clone)]
struct ElementParsePlan {
    metadata_name: String,
    kind: ElementParseKind,
}

#[derive(Clone)]
enum ElementParseKind {
    Fixed {
        expected: ExpectedSet,
        capture: PlannedCapturePolicy,
    },
    Semantic {
        field: FieldPlan,
        terminal: Box<Type>,
        wrappers: Vec<FieldWrapper>,
        container: Box<PlannedContainer>,
        capture: PlannedCapturePolicy,
        child_capture: PlannedCapturePolicy,
    },
}

#[derive(Clone)]
struct FieldPlan {
    ident: Option<Ident>,
    index: usize,
    accessor: Option<Ident>,
}

#[derive(Clone, Copy)]
enum FieldWrapper {
    Boxed,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone)]
enum PlannedContainer {
    Direct {
        child_follow: PlannedFollow,
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
    },
}

#[derive(Clone)]
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
    expected: ExpectedSet,
    capture: PlannedCapturePolicy,
}

#[derive(Clone)]
struct TokenBindingPlan {
    module: ModuleId,
    ident: Ident,
    visibility: Visibility,
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

    fn singleton(kind: usize, atom: u32, kind_count: usize) -> Self {
        let mut value = Self::empty(kind_count);
        value.words[kind / 64] |= 1_u64 << (kind % 64);
        value.atoms.push(atom);
        value
    }

    fn from_positions(positions: &[FirstPosition], kind_count: usize) -> Result<Self, String> {
        let mut value = Self::empty(kind_count);
        for position in positions {
            if position.text_predicate.is_some() {
                return Err(
                    "compound generation leaves text-predicate dispatch to ticket #67".to_owned(),
                );
            }
            for (left, right) in value.words.iter_mut().zip(position.kinds.words()) {
                *left |= *right;
            }
        }
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
        self.words.iter().all(|word| *word == 0)
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
}

fn fixed_lexical_pattern(spelling: &str, equivalence: TextEquivalence) -> String {
    let mut characters = spelling.chars();
    let word_shaped = characters.next().is_some_and(|first| {
        (first == '_' || unicode_ident::is_xid_start(first))
            && characters.all(unicode_ident::is_xid_continue)
    });
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
        let kind_count = lowered.grammar.kind_count + 1;
        let lexical_error_kind = u16::try_from(lowered.grammar.kind_count)
            .map_err(|_| "generated lexical-error kind exceeds runtime width".to_owned())?;
        let kind_variants = planned_kind_variants(lowered)?;
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
            let mut expected = ExpectedSet::from_kinds(&target.accepted_kinds(), kind_count);
            expected.atoms.push(atom.index().try_into().map_err(|_| {
                "generated content-token expectation exceeds runtime width".to_owned()
            })?);
            tokens.push(TokenParsePlan {
                ident: format_ident!("__RecursaContentToken{index}"),
                kind,
                kind_variant: kind_variants[target.base_kind.index()].clone(),
                expected,
                capture: resolved.build_inputs.spans.into(),
            });
        }
        let token_bindings = resolved
            .token_bindings
            .iter()
            .map(|binding| {
                Ok(TokenBindingPlan {
                    module: binding.module,
                    ident: emitted_ident(&binding.ident)?,
                    visibility: binding.visibility.clone(),
                    token: binding.target.index(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut nodes = Vec::new();
        for node in &resolved.nodes {
            let applications = lowered
                .applications
                .iter()
                .filter(|application| application.node == node.id)
                .collect::<Vec<_>>();
            let [application] = applications.as_slice() else {
                return Err(format!(
                    "Node `{}` must have exactly one materialized application before compound emission",
                    node.ident
                ));
            };
            nodes.push(plan_node(
                node,
                application,
                lowered,
                decisions,
                kind_count,
            )?);
        }

        // Keep fixed rules explicit in the executable tracer bullet even when
        // M3 proved that a later keyword-classification pass could eliminate
        // them. That post-lex optimization is not part of this runtime seam.
        let mut lexical_rules = lowered
            .source_modeled
            .source_model()
            .token_definitions
            .iter()
            .filter_map(|definition| {
                let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                    return None;
                };
                let correlation = lowered
                    .lexical_plan
                    .lowering_map
                    .token_definitions
                    .iter()
                    .find(|correlation| correlation.source == definition.id)?;
                Some(LexRulePlan {
                    pattern: fixed_lexical_pattern(
                        &fixed.spelling,
                        resolved.build_inputs.text_equivalence,
                    ),
                    kind: correlation.lowered.index().try_into().ok()?,
                    skip: false,
                })
            })
            .collect::<Vec<_>>();
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
            });
        }

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
                ";token={}:{}:{}",
                token.ident,
                token.kind,
                token.capture.canonical_name()
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
                ";rule={:?}:{}:{}",
                rule.pattern, rule.kind, rule.skip
            ));
        }
        let identity = format!("g_{:016x}", stable_hash(canonical.as_bytes()));
        let plan = Self {
            identity,
            facade,
            module_anchor,
            grammar_name: lowered.grammar.name.clone(),
            kind_count,
            lexical_error_kind,
            kind_variants,
            kind_names,
            atom_names,
            nodes,
            tokens,
            token_bindings,
            lexical_rules,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<(), String> {
        if self.identity.is_empty() || self.nodes.is_empty() {
            return Err("the Emission plan lacks a Generation identity or Nodes".to_owned());
        }
        if self.kind_variants.len() + 1 != self.kind_count
            || self.kind_names.len() != self.kind_count
        {
            return Err("the generated token-kind plan has inconsistent width".to_owned());
        }
        if self.lexical_rules.is_empty() {
            return Err("the generated lexical plan has no rules".to_owned());
        }
        Ok(())
    }
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

fn plan_node(
    node: &ResolvedNode,
    application: &MaterializedNodeApplication,
    lowered: &crate::materialization::LoweredGrammar,
    decisions: &crate::DecisionProducts,
    kind_count: usize,
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
            if dispatch.required_lookahead() > 1 {
                return Err(format!(
                    "enum Node `{}` requires FIRST-k dispatch owned by ticket #67",
                    node.ident
                ));
            }
            let mut planned = Vec::new();
            let mut expected = ExpectedSet::empty(kind_count);
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
                let mut selector = ExpectedSet::empty(kind_count);
                for sequence in branch.sequences() {
                    if sequence.positions().len() > 1 {
                        return Err(format!(
                            "enum variant `{}` requires FIRST-k dispatch owned by ticket #67",
                            variant.name
                        ));
                    }
                    selector.union_assign(&ExpectedSet::from_positions(
                        sequence.positions(),
                        kind_count,
                    )?);
                }
                if selector.is_empty() {
                    return Err(format!(
                        "nullable enum variant `{}` requires caller-FOLLOW dispatch beyond ticket #64",
                        variant.name
                    ));
                }
                expected.union_assign(&selector);
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
                    )?,
                    selector,
                    capture: resolved_variant.capture.map(PlannedCapturePolicy::from),
                });
            }
            NodeBodyPlan::Enum {
                variants: planned,
                expected,
            }
        }
        (MaterializedNodeBody::Pratt(_), _, _) => {
            return Err(format!(
                "Pratt Node `{}` is emitted by ticket #66, not compound ticket #64",
                node.ident
            ));
        }
        _ => {
            return Err(format!(
                "Node `{}` source shape and frozen M2 shape disagree",
                node.ident
            ));
        }
    };
    validate_accessor_names(node, &body)?;
    Ok(NodeParsePlan {
        semantic_key: node.key.segments.join("::"),
        module: node.module,
        ident: emitted_ident(&node.ident)?,
        visibility: node.visibility.clone(),
        loader_anchor,
        generics: node.generics.clone(),
        input_lifetime,
        body,
        capture: inherited_capture.into(),
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
        .map(|(element, site)| {
            let m2_site = &lowered.grammar.sites[site.index()];
            match element {
                MaterializedElement::FixedToken { token_use } => {
                    let source_model = lowered.source_modeled.source_model();
                    let token_use = &source_model.token_uses[token_use.index()];
                    let definition = &source_model.token_definitions[token_use.definition.index()];
                    let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                        return Err(
                            "a fixed element did not reference fixed-token syntax".to_owned()
                        );
                    };
                    let kind = lowered
                        .lexical_plan
                        .lowering_map
                        .token_definitions
                        .iter()
                        .find(|correlation| correlation.source == definition.id)
                        .ok_or_else(|| "a fixed element lacks a lexical kind".to_owned())?
                        .lowered
                        .index();
                    let atom = token_atom(lowered, m2_site.target)?;
                    let name = fixed_element_name(node, token_use, definition, fixed);
                    let capture = fixed_element_field(node, token_use)
                        .and_then(|field| field.capture)
                        .map(PlannedCapturePolicy::from)
                        .unwrap_or(variant_capture);
                    Ok(ElementParsePlan {
                        metadata_name: name,
                        kind: ElementParseKind::Fixed {
                            expected: ExpectedSet::singleton(kind, atom, kind_count),
                            capture,
                        },
                    })
                }
                MaterializedElement::SemanticField { field, ty, .. } => {
                    let resolved_field = node
                        .fields
                        .iter()
                        .find(|candidate| candidate.id == *field)
                        .ok_or_else(|| {
                            "materialized semantic field lacks source syntax".to_owned()
                        })?;
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
                        .ok_or_else(|| {
                            format!("field `{field_name}` lacks its container machine")
                        })?;
                    validate_container_wrappers(machine.operation(), &wrappers, &field_name)?;
                    let container = Box::new(planned_container(machine, kind_count)?);
                    Ok(ElementParsePlan {
                        metadata_name,
                        kind: ElementParseKind::Semantic {
                            field: FieldPlan {
                                ident: resolved_field.syntax.ident.clone(),
                                index: field_index,
                                accessor: Some({
                                    let base =
                                        resolved_field.syntax.ident.clone().unwrap_or_else(|| {
                                            format_ident!("field_{field_index}")
                                        });
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
                            container,
                            capture,
                            child_capture,
                        },
                    })
                }
            }
        })
        .collect()
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

fn token_atom(
    lowered: &crate::materialization::LoweredGrammar,
    target: crate::TypeId,
) -> Result<u32, String> {
    let TypeShape::Token { atom, .. } = &lowered.grammar.types[target.index()].shape else {
        return Err("a fixed element target did not lower to a token type".to_owned());
    };
    atom.index()
        .try_into()
        .map_err(|_| "generated fixed-token expectation exceeds runtime width".to_owned())
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
    kind_count: usize,
) -> Result<PlannedContainer, String> {
    let stop = planned_follow(machine.stop(), kind_count)?;
    let element_selector = ExpectedSet::from_positions(machine.element_first(), kind_count)?;
    Ok(match machine.operation() {
        ContainerOperation::Direct => PlannedContainer::Direct { child_follow: stop },
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
            }
        }
    })
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
        ClosedType::Node(node_use) => {
            let application = &lowered.applications[node_use.application.index()];
            let mut path =
                syn::parse_str::<syn::Path>(&format!("crate::{}", application.display_name))
                    .map_err(|error| {
                        format!(
                            "materialized terminal `{}` is not a Rust type path: {error}",
                            application.display_name
                        )
                    })?;
            if !node_use.lifetimes.is_empty() {
                let segment = path
                    .segments
                    .last_mut()
                    .ok_or_else(|| "materialized terminal path is empty".to_owned())?;
                let mut arguments = syn::punctuated::Punctuated::new();
                for lifetime in &node_use.lifetimes {
                    arguments.push(GenericArgument::Lifetime(lifetime.clone()));
                }
                match &segment.arguments {
                    PathArguments::None => {}
                    PathArguments::AngleBracketed(existing) => {
                        arguments.extend(existing.args.clone());
                    }
                    PathArguments::Parenthesized(_) => {
                        return Err("materialized terminal uses parenthesized arguments".to_owned());
                    }
                }
                segment.arguments =
                    PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                        colon2_token: None,
                        lt_token: Default::default(),
                        args: arguments,
                        gt_token: Default::default(),
                    });
            }
            Ok(Type::Path(syn::TypePath { qself: None, path }))
        }
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
                ident: format_ident!("__RecursaContentToken{}", token.index()),
                arguments: PathArguments::AngleBracketed(syn::parse_quote!(<#input_lifetime>)),
            });
            Ok(Type::Path(syn::TypePath { qself: None, path }))
        }
    }
}

fn planned_follow(recipe: &FollowRecipe, kind_count: usize) -> Result<PlannedFollow, String> {
    Ok(match recipe {
        FollowRecipe::Caller => PlannedFollow::Caller,
        FollowRecipe::Static { first } => {
            PlannedFollow::Static(ExpectedSet::from_positions(first, kind_count)?)
        }
        FollowRecipe::StaticAndCaller { first } => {
            PlannedFollow::StaticAndCaller(ExpectedSet::from_positions(first, kind_count)?)
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
            ElementParseKind::Fixed { .. } => fixed_accessor_ident(&element.metadata_name)?
                .unraw()
                .to_string(),
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
    }
    Ok(())
}

struct RenderedGeneration {
    root: String,
    nodes: Vec<(String, String)>,
    selector: String,
}

fn render(plan: &EmissionPlan) -> Result<RenderedGeneration, String> {
    plan.validate()?;
    let root = render_root(plan).to_string();
    let nodes = plan
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let name = format!("node-{index}.rs");
            let rendered = render_node(plan, node, index).to_string();
            syn::parse_file(&rendered)
                .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
            Ok((name, rendered))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let selected_root = format!("/recursa-generations/{}/root.rs", plan.identity);
    let selector = quote! {
        include!(concat!(env!("OUT_DIR"), #selected_root));
    }
    .to_string();
    syn::parse_file(&root).map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    syn::parse_file(&selector)
        .map_err(|error| format!("Generation selector rendering is invalid Rust: {error}"))?;
    Ok(RenderedGeneration {
        root,
        nodes,
        selector,
    })
}

fn render_root(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let release_text = env!("CARGO_PKG_VERSION");
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let protocol = format_ident!("protocol_{PRIVATE_PROTOCOL}");
    let grammar_name = &plan.grammar_name;
    let kind_count = plan.kind_count;
    let lexical_error_kind = plan.lexical_error_kind;
    let kind_names = &plan.kind_names;
    let atom_names = &plan.atom_names;
    let kind_variants = &plan.kind_variants;
    let kind_match_arms = kind_variants.iter().enumerate().map(|(index, variant)| {
        let index = index as u16;
        quote!(#index => TokenKind::#variant,)
    });
    let patterns = plan.lexical_rules.iter().map(|rule| &rule.pattern);
    let kinds = plan.lexical_rules.iter().map(|rule| rule.kind);
    let skips = plan.lexical_rules.iter().map(|rule| rule.skip);
    let empty_words = vec![0_u64; kind_count.div_ceil(64)];
    let token_starts = plan.tokens.iter().enumerate().map(|(index, token)| {
        let name = format_ident!("__RECURSA_TOKEN_START_{index}");
        render_expected_static(&name, &token.expected, kind_count, facade)
    });
    let token_items = plan
        .tokens
        .iter()
        .enumerate()
        .map(|(index, token)| render_token(plan, token, index));
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

    quote! {
        const _: #facade::__private::FacadeCompatibility<
            { #facade::__private::release_stamp(#release_text) },
            #PRIVATE_PROTOCOL,
        > = #facade::__private::FACADE_COMPATIBILITY;

        #[doc(hidden)]
        pub(crate) struct __RecursaGenerationAnchor;
        const _: fn() = || {
            let _: #module_anchor::__RecursaGenerationAnchor = __RecursaGenerationAnchor;
        };

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
        static __RECURSA_LEX_RULES: &[#facade::__private::LexRule] = &[
            #(
                #facade::__private::LexRule {
                    pattern: #patterns,
                    kind: #kinds,
                    skip: #skips,
                }
            ),*
        ];

        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum TokenKind {
            #(#kind_variants,)*
            LexicalError,
        }

        /// Read-only metadata for this generated grammar.
        pub struct GrammarInfo {
            _private: (),
        }

        impl GrammarInfo {
            pub fn name(&self) -> &'static str {
                __RECURSA_GRAMMAR.name()
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
                match self.record.kind {
                    #(#kind_match_arms)*
                    _ => TokenKind::LexicalError,
                }
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

        pub struct Tokens<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
        }

        impl<'input> Iterator for Tokens<'_, 'input> {
            type Item = Token<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records.next().copied().map(|record| Token {
                    source: self.source,
                    record,
                })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                self.records.size_hint()
            }
        }

        impl ExactSizeIterator for Tokens<'_, '_> {}

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

        pub struct LexResult<'input> {
            inner: #facade::__private::LexResult<'input>,
        }

        impl<'input> LexResult<'input> {
            pub fn tokens(&self) -> Tokens<'_, 'input> {
                Tokens {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                }
            }

            pub fn source(&self) -> &'input str {
                #facade::__private::lex_source(&self.inner)
            }

            pub fn input(&self) -> Input<'input> {
                Input {
                    inner: self.inner.input(&__RECURSA_GRAMMAR),
                }
            }

            pub fn input_with_builder<A>(&self, builder: A) -> Input<'input, A> {
                Input {
                    inner: self.inner.input_with_builder(&__RECURSA_GRAMMAR, builder),
                }
            }
        }

        pub fn lex(source: &str) -> LexResult<'_> {
            LexResult {
                inner: #facade::__private::lex(
                    source,
                    __RECURSA_LEX_RULES,
                    #lexical_error_kind,
                ),
            }
        }

        #(#token_items)*

        macro_rules! __recursa_root_handshake {
            (#release, #protocol, core) => {};
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
    let expected = format_ident!("__RECURSA_TOKEN_START_{index}");
    let capture = token.capture.render(facade);
    quote! {
        #[doc(hidden)]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct #ident<'input> {
            text: #facade::__private::ContentText<'input>,
        }

        impl<'input> #ident<'input> {
            pub fn new(text: impl Into<Box<str>>) -> Self {
                Self { text: #facade::__private::ContentText::owned(text.into()) }
            }

            pub fn text(&self) -> &str { self.text.as_str() }
            pub const fn kind(&self) -> #module_anchor::TokenKind {
                #module_anchor::TokenKind::#kind_variant
            }
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
                context.finish(Self { text }, record.span)
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
    quote! {
        #visibility type #ident<'input> = #module_anchor::#canonical<'input>;
        #visibility type #view<'view, 'input> =
            #module_anchor::#canonical_view<'view, 'input>;
    }
}

fn render_node(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
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
    let mut parse_generics = generics.clone();
    if !generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        parse_generics
            .params
            .insert(0, syn::parse_quote!('__recursa_input));
    }
    parse_generics.params.push(syn::parse_quote!(A));
    let (parse_impl_generics, _, parse_where_clause) = parse_generics.split_for_impl();
    let input_lifetime = &node.input_lifetime;
    let inherent_method_generics = if generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        quote!(<A>)
    } else {
        quote!(<'__recursa_input, A>)
    };
    let node_capture = node.capture.render(facade);
    let metadata = render_metadata(plan, node, node_index);
    let accessors = render_accessors(plan, node);
    let span_elements = render_span_elements(node, node_index);
    let parse_body = render_node_parse_body(plan, node, node_index);
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

        impl #impl_generics #facade::__private::NodeViewFactory for #node_ident #type_generics
            #where_clause
        {
            type View<'view> = #view_application where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> { #view_ident { value: self, cursor } }
        }

        impl #parse_impl_generics #facade::__private::Parse<#input_lifetime, A>
            for #node_ident #type_generics #parse_where_clause
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
            for #node_ident #type_generics #parse_where_clause
        {}

        impl #parse_impl_generics #facade::Parse<#module_anchor::Input<#input_lifetime, A>>
            for #node_ident #type_generics #parse_where_clause
        {
            type Output = #facade::Parsed<#input_lifetime, Self>;
            fn parse(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }

        #[allow(dead_code)]
        impl #impl_generics #node_ident #type_generics #where_clause {
            pub fn parse #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError> {
                <Self as #facade::Parse<#module_anchor::Input<#input_lifetime, A>>>::parse(input)
            }
            pub fn parse_spanned #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError> {
                #facade::__private::parse_spanned(&mut input.inner)
            }
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
    }
}

fn render_accessor(
    facade: &Ident,
    element: &ElementParsePlan,
    element_index: usize,
    variant: Option<(&Ident, &Ident, ResolvedFieldStyle)>,
) -> Option<proc_macro2::TokenStream> {
    match &element.kind {
        ElementParseKind::Fixed { .. } => {
            let method = fixed_accessor_ident(&element.metadata_name)
                .expect("fixed accessor names were validated during planning");
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
                ));
            }
            let is_optional = wrappers
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
                let value = if is_optional {
                    let optional_reference =
                        render_optional_view_reference(quote!(#binding), wrappers, true);
                    quote! {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                self.cursor.child(#element_index).child(0),
                            )
                        })
                    }
                } else {
                    let reference = render_terminal_reference(quote!(#binding), wrappers, true);
                    quote! {
                        Some(#facade::__private::NodeViewFactory::view(
                            #reference,
                            self.cursor.child(#element_index).child(0),
                        ))
                    }
                };
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view> {
                        match self.value {
                            #active_pattern => #value,
                            _ => None,
                        }
                    }
                })
            } else if is_optional {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let optional_reference = render_optional_view_reference(reference, wrappers, false);
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view> {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                self.cursor.child(#element_index).child(0),
                            )
                        })
                    }
                })
            } else {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let reference = render_terminal_reference(reference, wrappers, false);
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> impl #facade::NodeView<Value = #terminal> + 'view {
                        #facade::__private::NodeViewFactory::view(
                            #reference,
                            self.cursor.child(#element_index).child(0),
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
) -> proc_macro2::TokenStream {
    let outer = &wrappers[..repeat_index];
    let inner = &wrappers[repeat_index + 1..];
    let item_type = render_wrapped_type(facade, terminal, inner);
    let project = render_terminal_reference(quote!(value), inner, true);
    let cursor = quote!(self.cursor.child(#element_index));
    let repeated = |collection: proc_macro2::TokenStream| {
        quote! {
            #facade::Repeated::new(
                (#collection).as_slice(),
                #cursor,
                |value: &#item_type| -> &#terminal { #project },
            )
        }
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
            ) -> Option<#facade::Repeated<'view, #terminal, #item_type>> {
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
                ) -> Option<#facade::Repeated<'view, #terminal, #item_type>> {
                    #collection.map(|collection| #repeated)
                }
            }
        } else {
            let collection = render_terminal_reference(reference, outer, false);
            let repeated = repeated(collection);
            quote! {
                pub fn #accessor(
                    &self,
                ) -> #facade::Repeated<'view, #terminal, #item_type> {
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
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            render_sequence_parse(plan, node_index, None, *style, elements, None)
        }
        NodeBodyPlan::Enum { variants, expected } => {
            let expected_name = expected_static(node_index, usize::MAX, "dispatch");
            let expected_definition =
                render_expected_static(&expected_name, expected, plan.kind_count, facade);
            let branches = variants.iter().enumerate().map(|(index, variant)| {
                let selector_name = expected_static(node_index, index, "selector");
                let selector_definition = render_expected_static(
                    &selector_name,
                    &variant.selector,
                    plan.kind_count,
                    facade,
                );
                let body = render_sequence_parse(
                    plan,
                    node_index,
                    Some((index, &variant.ident)),
                    variant.style,
                    &variant.elements,
                    variant.capture,
                );
                quote! {
                    #selector_definition
                    if context.next_is(&#selector_name) { return { #body }; }
                }
            });
            quote! {
                #expected_definition
                #(#branches)*
                Err(context.unexpected(&#expected_name))
            }
        }
    }
}

fn render_sequence_parse(
    plan: &EmissionPlan,
    node_index: usize,
    variant: Option<(usize, &Ident)>,
    style: ResolvedFieldStyle,
    elements: &[ElementParsePlan],
    capture: Option<PlannedCapturePolicy>,
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
            ElementParseKind::Fixed { expected, capture } => {
                let expected_name = expected_static(node_index, unique_index, "fixed");
                let expected_definition =
                    render_expected_static(&expected_name, expected, plan.kind_count, facade);
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
            let span = context
                .consumed_span_from(#start)
                .expect("a successful non-empty Node parse consumes one valid token span");
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
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let capture = capture.render(facade);
    let child_capture = child_capture.render(facade);
    match container {
        PlannedContainer::Direct { child_follow } => {
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            quote! {
                #follow_definition
                let element_start = context.cursor();
                let (parsed_value, child_occurrence) = context
                    .parse_node::<#terminal>(&#follow_value, #child_capture)?
                    .into_parts();
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
                render_expected_static(&selector_name, selector, plan.kind_count, facade);
            let follow_name = expected_static(node_index, unique_index, "child_follow");
            let (follow_definition, follow_value) =
                render_follow_value(plan, &follow_name, child_follow);
            let wrapped = render_wrapped_value(quote!(parsed_value), wrappers);
            let absent = render_absent_value(wrappers);
            quote! {
                #selector_definition
                #follow_definition
                let (#value, #occurrence) = if context.next_is(&#selector_name) {
                    let element_start = context.cursor();
                    let (parsed_value, child_occurrence) = context
                        .parse_node::<#terminal>(&#follow_value, #child_capture)?
                        .into_parts();
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
            capture,
            child_capture,
        ),
    }
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
    capture: proc_macro2::TokenStream,
    child_capture: proc_macro2::TokenStream,
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
    let element_definition =
        render_expected_static(&element_name, element_selector, plan.kind_count, facade);
    let stop_name = expected_static(node_index, unique_index, "stop");
    let (stop_definition, stop_value) = render_follow_value(plan, &stop_name, stop);
    let child_follow_name = expected_static(node_index, unique_index, "child_follow");
    let (child_follow_definition, child_follow_value) =
        render_follow_value(plan, &child_follow_name, child_follow);
    let parse_item = quote! {
        let (parsed_value, child_occurrence) = context
            .parse_node::<#terminal>(&#child_follow_value, #child_capture)?
            .into_parts();
        parsed_values.push(#wrapped_item);
        child_occurrences.push(child_occurrence);
    };
    let required = matches!(operation, ContainerOperation::OneOrMore);
    let start = if required {
        quote! {
            if !context.next_is(&#element_name) {
                return Err(context.unexpected(&#element_name));
            }
            #parse_item
        }
    } else {
        quote! {
            if context.next_is(&#element_name) {
                #parse_item
            } else if !context.is_eof() && !context.next_is(&#stop_value) {
                return Err(context.unexpected(&#element_name));
            }
        }
    };

    let continuation = if let Some(separator_selector) = separator_selector {
        let separator_name = expected_static(node_index, unique_index, "separator");
        let separator_definition =
            render_expected_static(&separator_name, separator_selector, plan.kind_count, facade);
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
                    return Err(context.unexpected(&#child_follow_value));
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
                return Err(context.unexpected(&#element_name));
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
                return Err(context.unexpected(&#child_follow_value));
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
            let definition = render_expected_static(name, expected, plan.kind_count, facade);
            (
                quote!(#definition let #value = #name.clone();),
                quote!(#value),
            )
        }
        PlannedFollow::StaticAndCaller(expected) => {
            let definition = render_expected_static(name, expected, plan.kind_count, facade);
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
    kind_count: usize,
    facade: &Ident,
) -> proc_macro2::TokenStream {
    let words = &expected.words;
    let atoms = expected
        .atoms
        .iter()
        .map(|atom| quote!(#facade::__private::ExpectedAtomId(#atom)));
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
    let generations = out_dir.join("recursa-generations");
    fs::create_dir_all(&generations)
        .map_err(|error| format!("cannot create Generation directory: {error}"))?;
    let published = generations.join(identity);
    if published.exists() {
        if !published.join("root.rs").is_file()
            || rendered
                .nodes
                .iter()
                .any(|(name, _)| !published.join(name).is_file())
        {
            return Err("an existing published Generation is incomplete".to_owned());
        }
    } else {
        let nonce = staging_nonce()?;
        let staging = generations.join(format!(
            ".staging-{}-{nonce}-{identity}",
            std::process::id()
        ));
        fs::create_dir(&staging)
            .map_err(|error| format!("cannot create Generation staging directory: {error}"))?;
        stage_and_publish(&staging, &published, rendered, || Ok(()))?;
    }
    select_generation(out_dir, rendered)
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
    fs::rename(staging, published)
        .map_err(|error| format!("cannot cross the Generation publication barrier: {error}"))?;
    Ok(())
}

fn select_generation(out_dir: &Path, rendered: &RenderedGeneration) -> Result<(), String> {
    let nonce = staging_nonce()?;
    let staging = out_dir.join(format!(
        ".recursa-selector-{}-{nonce}.rs",
        std::process::id()
    ));
    fs::write(&staging, &rendered.selector)
        .map_err(|error| format!("cannot stage the Generation selector: {error}"))?;
    fs::rename(&staging, out_dir.join(SELECTED_GENERATION))
        .map_err(|error| format!("cannot atomically select the complete Generation: {error}"))
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
    fn snake_case_produces_stable_generated_element_names() {
        assert_eq!(snake_case("RECORD"), "record");
        assert_eq!(snake_case("LabelColon"), "label_colon");
        assert_eq!(snake_case(":"), "element");
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
    fn incomplete_staging_is_unselectable() {
        let output = tempfile::tempdir().unwrap();
        let staging = output.path().join("staging");
        let published = output.path().join("published");
        fs::create_dir(&staging).unwrap();
        let rendered = RenderedGeneration {
            root: "fn root() {}".to_owned(),
            nodes: vec![("node-0.rs".to_owned(), "fn node() {}".to_owned())],
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
}
